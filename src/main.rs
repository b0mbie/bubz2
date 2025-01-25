use bzip2::{
	Compression,
	write::BzEncoder,
};
use pico_args::Arguments;
use rustc_hash::{
	FxHashMap, FxBuildHasher,
};
use slicepat::{
	PathMatch,
	u8_buf::U8Pieces,
};
use std::{
	borrow::Cow,
	env::args_os,
	fs::{
		create_dir_all, File,
	},
	hash::Hash,
	io::{
		Error as IoError, ErrorKind as IoErrorKind, Result as IoResult,
		BufReader,
		Read, Write, BufRead,
	},
	ops::{
		Deref, DerefMut,
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

type Pattern = slicepat::Pattern<U8Pieces, u8>;

fn main() -> ExitCode {
	macro_rules! err_or_return {
		($expr:expr; $e:pat => $($fmt:tt)*) => {
			match $expr {
				Ok(v) => v,
				Err($e) => {
					eprintln!($($fmt)*);
					return ExitCode::FAILURE
				}
			}
		};

		($expr:expr) => {
			err_or_return!($expr; e => "{e}")
		};
	}

	let args: Vec<_> = args_os().collect();
	let args_empty = args.len() <= 1;
	let mut args = Arguments::from_vec(args);
	if args_empty || args.contains(["-h", "--help"]) {
		eprint!("\
{} {} - {}

--from <path>:
	Source directory, with uncompressed files.
--to <path>:
	Destination directory, to be filled with compressed files.
--state <path>:
	Defaults to `--state .fastdl`.
	Path to file describing the last modified times that were seen
	previously of uncompressed files.
--ignore <path>:
	Path to file containing wildcard patterns for source file paths
	that must not be compressed (excluded).

	Each pattern is defined on a separate line, with a `*` symbol
	denoting that any character before the sequence after it is
	accepted.
	Lines, trimmed of whitespace, beginning with `#`, denote
	comments.
	Patterns beginning with `!` match files that are to always be
	included.
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

	let source_dir: PathBuf = err_or_return!(args.value_from_str("--from"));
	let dest_dir: PathBuf = err_or_return!(args.value_from_str("--to"));

	let state_path: Cow<'static, Path> = err_or_return!(args.opt_value_from_str("--state"))
		.map(Cow::Owned)
		.unwrap_or(Cow::Borrowed(Path::new(".fastdl")));

	let mut state = {
		let file = File::options()
			.create(true).truncate(false)
			.read(true).write(true)
			.open(&state_path);
		let file = err_or_return!(file; e => "Couldn't open state file {state_path:?}: {e}");
		State::empty(file)
	};

	err_or_return!(state.read_all(); e => "Couldn't read from state file {state_path:?}: {e}");

	let mut to_compress = Vec::new();
	struct ToCompress {
		pub source_path: PathBuf,
		pub destination_path: PathBuf,
	}

	let compression = {
		let level: Cow<'static, str> = err_or_return!(args.opt_value_from_str("--level"))
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

	let ignore_patterns = {
		let mut map = PatternMap::new();
		if let Some(path) = err_or_return!(args.opt_value_from_str::<_, PathBuf>("--ignore")) {
			match File::open(path) {
				Ok(f) => {
					err_or_return!(
						map.read_from(BufReader::new(f));
						e => "Failed to read ignore file: {e}"
					);
				}
				Err(e) if e.kind() == IoErrorKind::NotFound => {}
				Err(e) => {
					eprintln!("Failed to open ignore file: {e}");
					return ExitCode::FAILURE
				}
			};
		}
		map
	};

	let mut to_traverse = vec![source_dir.clone()];
	while let Some(dir) = to_traverse.pop() {
		let items = err_or_return!(dir.read_dir(); e => "Couldn't read directory {dir:?}: {e}");

		for item in items.flatten() {
			let source_path = item.path();
			let relative_path = source_path.strip_prefix(&source_dir)
				.expect("`item.path()` returns with prefix of `dir`");

			if ignore_patterns.has_match(relative_path.as_os_str().as_encoded_bytes()) {
				println!("!{}", source_path.display());
				continue
			}

			let metadata = err_or_return!(item.metadata(); e => "Couldn't get metadata for {source_path:?}: {e}");

			if metadata.is_dir() {
				to_traverse.push(source_path);
			} else {
				let fs_time = metadata.modified()
					.expect("last modification time should be supported")
					.duration_since(SystemTime::UNIX_EPOCH)
					.expect("system clock should be past the Unix epoch")
					.as_secs();

				let mut destination_path = dest_dir.join(relative_path);
				if let Some(extension) = destination_path.extension() {
					let mut extension = extension.to_os_string();
					extension.push(".bz2");
					destination_path.set_extension(extension);
				} else {
					destination_path.set_extension("bz2");
				}

				if let Some(parent_path) = destination_path.parent() {
					err_or_return!(
						create_dir_all(parent_path);
						e => "Couldn't create parent directories for {destination_path:?}: {e}"
					);
				}

				if
					!destination_path.exists()
					|| state.time_of(relative_path) != Some(fs_time)
				{
					err_or_return!(
						state.set_time_of(relative_path, fs_time);
						e => "Couldn't write time for {source_path:?}: {e}"
					);

					to_compress.push(ToCompress {
						source_path,
						destination_path,
					});
				}
			}
		}
	}

	let rt = err_or_return!(Builder::new_multi_thread().build(); e => "Couldn't build async runtime: {e}");

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
					println!("{} => {}", source.display(), destination.display());
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

#[derive(Default, Debug, Clone)]
#[repr(transparent)]
pub struct PatternMap(pub FxHashMap<Pattern, Directive>);

impl PatternMap {
	#[inline]
	pub fn new() -> Self {
		Self(FxHashMap::with_hasher(FxBuildHasher))
	}

	pub fn has_match(&self, haystack: &[u8]) -> bool {
		let mut one_matched = false;
		for (pattern, directive) in self.0.iter() {
			if pattern.first_match(PathMatch, haystack).is_some() {
				match directive {
					Directive::Include => {
						one_matched = true;
					}
					Directive::Exclude => {
						return false
					}
				}
			}
		}
		one_matched
	}

	pub fn insert(&mut self, pattern: Pattern, directive: Directive) {
		self.0.insert(pattern, directive);
	}

	pub fn read_from<R: BufRead>(&mut self, mut r: R) -> IoResult<()> {
		struct ClearGuard<'a>(&'a mut String);
		impl Deref for ClearGuard<'_> {
			type Target = String;
			fn deref(&self) -> &Self::Target {
				self.0
			}
		}
		impl DerefMut for ClearGuard<'_> {
			fn deref_mut(&mut self) -> &mut Self::Target {
				self.0
			}
		}
		impl Drop for ClearGuard<'_> {
			fn drop(&mut self) {
				self.0.clear();
			}
		}

		let mut line = String::new();
		while r.read_line(&mut line)? != 0 {
			let line = ClearGuard(&mut line);
			let trimmed_line = line.trim();
			let Some((first, rest)) = trimmed_line.split_at_checked(1) else {
				// We skip a `trimmed_line.is_empty()` check this way, too.
				continue
			};
			
			let (pattern_str, directive) = match first {
				"#" => continue,
				"!" => (rest, Directive::Exclude),
				"\\" => (rest, Directive::Include),
				_ => (trimmed_line, Directive::Include),
			};

			let pattern = Pattern::parse(pattern_str.as_bytes(), &b'*');
			self.insert(pattern, directive);
		}

		Ok(())
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Directive {
	Include,
	Exclude,
}
