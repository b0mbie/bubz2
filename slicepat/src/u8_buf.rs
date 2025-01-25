use alloc::vec::Vec;
use core::{
	fmt,
	mem::size_of,
};

use crate::Pieces;

#[derive(Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct U8Pieces(Vec<u8>);

impl U8Pieces {
	pub const fn new() -> Self {
		Self(Vec::new())
	}

	pub fn with_capacity(capacity: usize) -> Self {
		Self(Vec::with_capacity(capacity))
	}
	
	pub fn push(&mut self, piece: &[u8]) {
		let piece_len = piece.len();
		if piece_len > 0 {
			let piece_len_buf = piece_len.to_ne_bytes();
			self.0.reserve(piece_len_buf.len() + piece_len);
			self.0.extend_from_slice(&piece_len_buf);
			self.0.extend_from_slice(piece);
		}
	}

	pub fn capacity(&self) -> usize {
		self.0.capacity()
	}

	pub fn reserve(&mut self, total_len: usize) {
		self.0.reserve(total_len);
	}
}

impl Pieces<u8> for U8Pieces {
	type Iter<'a> = U8PiecesIter<'a>;
	fn pieces(&self) -> Self::Iter<'_> {
		unsafe { U8PiecesIter::new_unchecked(self.0.as_slice()) }
	}
}

impl<'a> FromIterator<&'a [u8]> for U8Pieces {
	fn from_iter<T: IntoIterator<Item = &'a [u8]>>(iter: T) -> Self {
		let mut result = Self::new();
		for piece in iter {
			result.push(piece);
		}
		result
	}
}

impl<'a, T: AsRef<[&'a [u8]]>> From<T> for U8Pieces {
	fn from(value: T) -> Self {
		let capacity = value.as_ref().iter().map(move |piece| size_of::<usize>() + piece.len()).sum();
		let mut result = Self::with_capacity(capacity);
		for piece in value.as_ref() {
			result.push(piece);
		}
		result
	}
}

impl fmt::Debug for U8Pieces {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut list = f.debug_list();
		for piece in self.pieces() {
			list.entry(&piece);
		}
		list.finish()
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct U8PiecesIter<'a>(&'a [u8]);

impl<'a> U8PiecesIter<'a> {
	/// # Safety
	/// `inner` must be a slice that contains sequences of [`u8`]s, with each sequence prepended with its length in
	/// [`usize`] encoded in native-endian.
	#[inline]
	pub const unsafe fn new_unchecked(inner: &'a [u8]) -> Self {
		Self(inner)
	}
}

impl<'a> Iterator for U8PiecesIter<'a> {
	type Item = &'a [u8];
	fn next(&mut self) -> Option<Self::Item> {
		let (piece_len, after_len) = self.0.split_at_checked(size_of::<usize>())?;
		let piece_len = usize::from_ne_bytes(piece_len.try_into().ok()?);
		let piece;
		(piece, self.0) = after_len.split_at_checked(piece_len)?;
		Some(piece)
	}
}

#[test]
fn iter_buf_pieces() {
	let pieces_array = [b"one".as_ref(), b"tour"];
	let pieces = U8Pieces::from(pieces_array);
	assert_eq!(pieces.pieces().count(), pieces_array.len());
	assert_eq!(pieces.pieces().zip(pieces_array).find(move |(piece, orig_piece)| piece != orig_piece), None);
}
