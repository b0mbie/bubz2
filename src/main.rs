use bzip2::{
	Compression,
	write::BzEncoder,
};
use pico_args::Arguments;
use std::{
	borrow::Cow,
	fs::{
		create_dir_all, File,
	},
	io::{
		Error as IoError, Read, Write,
	},
	path::{
		Path, PathBuf
	},
	process::ExitCode,
	time::SystemTime,
};
use tokio::{
	runtime::Builder,
	task::JoinSet
};

mod state;
use state::*;

fn main() -> ExitCode {
	let mut args = Arguments::from_env();
	if args.contains(["-h", "--help"]) {
		eprint!("\
{} {} - {}

--slowdl <path>:
	Source directory, with uncompressed files.
--fastdl <path>:
	Destination directory, to be filled with compressed files.
--state <path>:
	Defaults to `--state .fastdl`.
	Path to file describing the last modified times that were seen
	previously of uncompressed files.
--level <compression level>:
	Defaults to `--level best`.
	Bzip2 compression level. Can be one of:
	- `none`: No compression.
	- `fast`: Optimized for best encoding speed.
	- `best`: Optimized for best file size.
	- `0` through `9`: Semi-arbitrary numeric level.
",
			env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"),
			env!("CARGO_PKG_DESCRIPTION"),
		);
		return ExitCode::SUCCESS
	}

	let Ok(slowdl_dir): Result<PathBuf, _> = args.value_from_str("--slowdl")
	else {
		eprintln!("Expected SlowDL directory (--slowdl).");
		return ExitCode::FAILURE
	};
	let Ok(fastdl_dir): Result<PathBuf, _> = args.value_from_str("--fastdl")
	else {
		eprintln!("Expected FastDL directory (--fastdl).");
		return ExitCode::FAILURE
	};

	let state_path: Cow<'static, Path> = args.value_from_str("--state")
		.map(Cow::Owned)
		.unwrap_or(Cow::Borrowed(Path::new(".fastdl")));

	let mut state = {
		let file = File::options()
			.create(true).truncate(false)
			.read(true).write(true)
			.open(state_path);
		let file = match file {
			Ok(file) => file,
			Err(e) => {
				eprintln!("Couldn't open state file: {e}");
				return ExitCode::FAILURE
			}
		};
		State::empty(file)
	};

	if let Err(e) = state.read_all() {
		eprintln!("Couldn't read from state file: {e}");
		return ExitCode::FAILURE
	}

	let mut to_compress = Vec::new();
	struct ToCompress {
		pub source_path: PathBuf,
		pub destination_path: PathBuf,
	}

	let mut to_traverse = vec![slowdl_dir.clone()];
	while let Some(dir) = to_traverse.pop() {
		let items = match dir.read_dir() {
			Ok(items) => items,
			Err(e) => {
				eprintln!("Couldn't read directory {dir:?}: {e}");
				return ExitCode::FAILURE
			}
		};

		for item in items.flatten() {
			let source_path = item.path();
			let metadata = match item.metadata() {
				Ok(metadata) => metadata,
				Err(e) => {
					eprintln!("Couldn't get metadata for {source_path:?}: {e}");
					return ExitCode::FAILURE
				}
			};

			if metadata.is_dir() {
				to_traverse.push(source_path);
			} else {
				let fs_time = metadata.modified()
					.expect("last modification time should be supported")
					.duration_since(SystemTime::UNIX_EPOCH)
					.expect("system clock should be past the Unix epoch")
					.as_secs();

				let relative_path = source_path.strip_prefix(&slowdl_dir)
					.expect("item.path() returns with prefix of `dir`");

				let mut destination_path = fastdl_dir.join(relative_path);
				if let Some(extension) = destination_path.extension() {
					let mut extension = extension.to_os_string();
					extension.push(".bz2");
					destination_path.set_extension(extension);
				} else {
					destination_path.set_extension(".bz2");
				}

				if let Some(parent_path) = destination_path.parent() {
					if let Err(e) = create_dir_all(parent_path) {
						eprintln!(
							"Couldn't create parent directories for {:?}: {e}",
							destination_path,
						);
						return ExitCode::FAILURE
					}
				}

				if
					!destination_path.exists()
					|| state.time_of(relative_path) != Some(fs_time)
				{
					if let Err(e) = state.set_time_of(relative_path, fs_time) {
						eprintln!("Couldn't write time for {source_path:?}: {e}");
						return ExitCode::FAILURE
					}

					to_compress.push(ToCompress {
						source_path,
						destination_path,
					});
				}
			}
		}
	}

	let compression = {
		let level: Cow<'static, str> = args.opt_value_from_str("--level").unwrap()
			.map(Cow::Owned)
			.unwrap_or(Cow::Borrowed("best"));
		match level.as_ref() {
			"none" => Compression::none(),
			"fast" => Compression::fast(),
			"best" => Compression::best(),
			"0" => Compression::new(0),
			"1" => Compression::new(1),
			"2" => Compression::new(2),
			"3" => Compression::new(3),
			"4" => Compression::new(4),
			"5" => Compression::new(5),
			"6" => Compression::new(6),
			"7" => Compression::new(7),
			"8" => Compression::new(8),
			"9" => Compression::new(9),
			level => {
				eprintln!("Invalid compression level {level:?}.");
				return ExitCode::FAILURE
			}
		}
	};

	let rt = match Builder::new_multi_thread().build() {
		Ok(rt) => rt,
		Err(e) => {
			eprintln!("Couldn't build async runtime: {e}");
			return ExitCode::FAILURE
		}
	};

	rt.block_on(async move {
		let mut task_set = JoinSet::new();
		for ToCompress { source_path, destination_path } in to_compress {
			task_set.spawn_blocking(move || {
				let destination = File::options()
					.create(true).truncate(true).write(true)
					.open(&destination_path)?;
				let mut destination = BzEncoder::new(destination, compression);
				let mut source = File::options().read(true).open(&source_path)?;
				let mut buffer = [0u8; 1024];
				while let Ok(n) = source.read(&mut buffer) {
					if n == 0 { break }
					destination.write_all(&buffer[..n])?;
				}
				destination.finish()?;
				Ok::<_, IoError>((source_path, destination_path))
			});
		}

		let mut failed = false;
		while let Some(join_result) = task_set.join_next().await {
			match join_result {
				Ok(Ok((source, destination))) => {
					println!("{source:?} => {destination:?}");
				}
				Ok(Err(e)) => {
					failed = true;
					eprintln!("{e}");
				}
				Err(e) => {
					failed = true;
					eprintln!("Couldn't join task, this is a bug: {e}");
				}
			}
		}

		if !failed { ExitCode::SUCCESS } else { ExitCode::FAILURE }
	})
}
