use core::marker::PhantomData;

use crate::{
	Matcher,
	matches_impl, suffix_matches_impl,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pattern<P, T> {
	pub flags: PatternFlags,
	pub pieces: P,
	pub piece_t: PhantomData<fn() -> T>,
}

impl<T, P: Pieces<T>> Pattern<P, T> {
	#[inline]
	pub const fn new(pieces: P, flags: PatternFlags) -> Self {
		Self {
			pieces,
			flags,
			piece_t: PhantomData,
		}
	}

	pub fn first_match<'a, M: Matcher<T>>(&self, matcher: M, haystack: &'a [T]) -> Option<&'a [T]> {
		let rest = if self.flags.is_start_unanchored() {
			suffix_matches_impl(self.pieces.pieces(), matcher, haystack)
		} else {
			matches_impl(self.pieces.pieces(), matcher, haystack)
		};
		rest.filter(move |rest| !self.flags.is_end_anchored() || rest.is_empty())
	}
}

impl<'a, T: 'a + PartialEq, P: FromIterator<&'a [T]>> Pattern<P, T> {
	pub fn parse(pattern: &'a [T], wildcard: &T) -> Self {
		let mut flags = PatternFlags::empty();
		if pattern.first() == Some(wildcard) {
			flags = flags.with_start_unanchored();
		}
		if pattern.last() != Some(wildcard) {
			flags = flags.with_end_anchored();
		}

		Self {
			flags,
			pieces: pattern.split(move |t| t == wildcard).collect(),
			piece_t: PhantomData,
		}
	}
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternFlags(u8);

impl PatternFlags {
	const FLAG_START_UNANCHORED: u8 = 1 << 0;
	const FLAG_END_ANCHORED: u8 = 1 << 1;

	#[inline]
	pub const fn empty() -> Self {
		Self(0)
	}

	#[inline]
	pub const fn is_start_unanchored(self) -> bool {
		(self.0 & Self::FLAG_START_UNANCHORED) != 0
	}

	#[must_use = "`with_start_unanchored` returns without modifying the original value"]
	#[inline]
	pub const fn with_start_unanchored(self) -> Self {
		Self(self.0 | Self::FLAG_START_UNANCHORED)
	}

	#[inline]
	pub const fn is_end_anchored(self) -> bool {
		(self.0 & Self::FLAG_END_ANCHORED) != 0
	}

	#[inline]
	#[must_use = "`with_end_anchored` returns without modifying the original value"]
	pub const fn with_end_anchored(self) -> Self {
		Self(self.0 | Self::FLAG_END_ANCHORED)
	}
}

pub trait Pieces<T> {
	type Iter<'a>: Iterator<Item = &'a [T]> where Self: 'a, T: 'a;
	fn pieces(&self) -> Self::Iter<'_>;
}

impl<T, P: Pieces<T>> Pieces<T> for &P {
	type Iter<'a> = P::Iter<'a> where Self: 'a, T: 'a;
	fn pieces(&self) -> Self::Iter<'_> {
		Pieces::pieces(*self)
	}
}

#[cfg(test)]
mod tests {
	use crate::*;
	use u8_buf::*;

	const WILDCARD: &u8 = &b'*';

	#[cfg(feature = "alloc")]
	#[test]
	fn pattern_parses() {
		assert_eq!(
			Pattern::parse(b"a*b", WILDCARD),
			Pattern::new(U8Pieces::from([b"a".as_ref(), b"b"]), PatternFlags::empty().with_end_anchored()),
		);
		assert_eq!(
			Pattern::parse(b"*a*b", WILDCARD),
			Pattern::new(
				U8Pieces::from([b"a".as_ref(), b"b"]),
				PatternFlags::empty().with_start_unanchored().with_end_anchored(),
			),
		);
		assert_eq!(
			Pattern::parse(b"*.Nav", WILDCARD),
			Pattern::new(
				U8Pieces::from([b".Nav".as_ref()]),
				PatternFlags::empty().with_start_unanchored().with_end_anchored(),
			),
		);
	}

	#[cfg(feature = "alloc")]
	#[test]
	fn pattern_matches() {
		let pattern: Pattern<U8Pieces, u8> = Pattern::parse(b"*.nav", WILDCARD);
		assert_eq!(pattern.first_match(PathMatch, b"DM_FLOOD.NAV"), Some(b"".as_ref()));
		assert_eq!(pattern.first_match(PathMatch, b"dm_flood.Nav"), Some(b"".as_ref()));
		assert_eq!(pattern.first_match(PathMatch, b"dm_flood.nav\t"), None);
		let pattern: Pattern<U8Pieces, u8> = Pattern::parse(b"*.nav*", WILDCARD);
		assert_eq!(pattern.first_match(PathMatch, b"cp_dustbowl.nav  "), Some(b"  ".as_ref()));
	}
}
