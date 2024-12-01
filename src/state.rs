use rustc_hash::{
	FxHashMap, FxBuildHasher
};
use std::{
	io::{
		SeekFrom,
		Write, Read, Seek,
		BufReader, BufRead,
		Error as IoError, ErrorKind as IoErrorKind,
	},
	path::{
		Path, PathBuf
	},
};

/// State object for keeping track of last modified times for files.
#[derive(Debug)]
pub struct State<F> {
	source: F,
	data: FxHashMap<PathBuf, StateValue>,
}

impl<F> State<F> {
	/// Create a state object with no time associations.
	pub fn empty(source: F) -> Self {
		Self {
			source,
			data: FxHashMap::with_hasher(FxBuildHasher),
		}
	}

	fn format_line(path: &Path, time: u64) -> Vec<u8> {
		let mut line = Vec::new();
		let _ = write!(line, "{time:08x},");
		line.extend_from_slice(path.as_os_str().as_encoded_bytes());
		line
	}

	fn parse_line(line: &str) -> Result<(PathBuf, u64), IoError> {
		let (secs, path) = line.split_once(',')
			.ok_or_else(move || IoError::new(
				IoErrorKind::InvalidData, "expected time and path"
			))?;
		let secs = u64::from_str_radix(secs, 16)
			.map_err(move |e| IoError::new(IoErrorKind::InvalidData, e))?;
		Ok((path.trim_end().into(), secs))
	}
}

impl<F: Seek + Write + Read> State<F> {
	/// Read all consequent entries in the source after the cursor.
	pub fn read_all_from_cur(&mut self) -> Result<(), IoError> {
		let mut buf_reader = BufReader::new(&mut self.source);
		let mut line = String::new();
		let mut offset = 0;
		loop {
			let length = buf_reader.read_line(&mut line)? as u64;
			if length == 0 {
				break Ok(())
			}
			let set_offset = offset;
			offset += length;

			let line_str = line.trim();
			if line_str.is_empty() {
				continue
			}

			let (path, time) = Self::parse_line(line_str)?;
			self.data.insert(path, StateValue {
				offset: set_offset, 
				time,
			});
			line.clear();
		}
	}

	/// Read all entries in the source from the beginning.
	pub fn read_all(&mut self) -> Result<(), IoError> {
		self.source.seek(SeekFrom::Start(0))?;
		self.read_all_from_cur()
	}

	/// Get the last modified time, expressed in seconds after the Unix epoch,
	/// associated with `path`.
	pub fn time_of(&self, path: &Path) -> Option<u64> {
		self.data.get(path).map(move |v| v.time)
	}

	/// Set the last modified time, expressed in seconds after the Unix epoch,
	/// associated with `path`, to `time`.
	pub fn set_time_of(
		&mut self, path: &Path, time: u64,
	) -> Result<(), IoError> {
		if let Some(value) = self.data.get_mut(path) {
			self.source.seek(SeekFrom::Start(value.offset))?;

			self.source.write_all(&Self::format_line(path, time))?;

			value.time = time;
			Ok(())
		} else {
			let offset = self.source.seek(SeekFrom::End(0))?;
			if offset != 0 {
				self.source.write_all(b"\n")?;
			}
			self.source.write_all(&Self::format_line(path, time))?;

			self.data.insert(path.to_path_buf(), StateValue {
				offset, time,
			});
			Ok(())
		}
	}
}

#[derive(Debug)]
struct StateValue {
	pub offset: u64,
	pub time: u64,
}
