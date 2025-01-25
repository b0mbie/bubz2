//! Slice patterns.

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
pub mod u8_buf;

mod pattern;
pub use pattern::*;

pub fn matches<'a, P, M, T: 'a>(pattern: P, matcher: M, haystack: &[T]) -> Option<&[T]>
where
	P: IntoIterator<Item = &'a [T]>,
	M: Matcher<T>,
{
	matches_impl(pattern.into_iter(), matcher, haystack)
}

pub fn suffix_matches<'a, P, M, T: 'a>(pattern: P, matcher: M, haystack: &[T]) -> Option<&[T]>
where
	P: IntoIterator<Item = &'a [T]>,
	M: Matcher<T>,
{
	suffix_matches_impl(pattern.into_iter(), matcher, haystack)
}

fn matches_impl<'a, P, M, T: 'a>(mut pattern: P, matcher: M, mut haystack: &[T]) -> Option<&[T]>
where
	P: Iterator<Item = &'a [T]>,
	M: Matcher<T>,
{
	match pattern.next() {
		Some(first) => {
			haystack = haystack.split_at_checked(first.len())
				.and_then(|(window, haystack)| matcher.is_equal(first, window).then_some(haystack))?;
		}
		None => return haystack.is_empty().then_some(haystack)
	}
	suffix_matches_impl(pattern, matcher, haystack)
}

fn suffix_matches_impl<'a, P, M, T: 'a>(pattern: P, matcher: M, mut haystack: &[T]) -> Option<&[T]>
where
	P: Iterator<Item = &'a [T]>,
	M: Matcher<T>,
{
	for piece in pattern {
		let piece_len = piece.len();
		if piece_len > 0 {
			let new_haystack = haystack.windows(piece_len)
				.position(|window| matcher.is_equal(piece, window))
				.and_then(move |offset| haystack.get(offset + piece_len..))?;
			/*
			let new_haystack = memchr::memmem::find(haystack, piece)
				.and_then(move |offset| haystack.get(offset + piece_len..));
			*/
			haystack = new_haystack;
		}
	}
	Some(haystack)
}

pub trait Matcher<T> {
	fn is_equal(&self, a: &[T], b: &[T]) -> bool;
}

impl<T, M: Matcher<T>> Matcher<T> for &M {
	fn is_equal(&self, a: &[T], b: &[T]) -> bool {
		Matcher::is_equal(*self, a, b)
	}
}

/*
impl<F: Fn(&[u8], &[u8]) -> bool> Matcher for F {
	fn is_equal(&self, a: &[u8], b: &[u8]) -> bool {
		self(a, b)
	}
}
*/

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExactMatch;
impl<T: PartialEq> Matcher<T> for ExactMatch {
	fn is_equal(&self, a: &[T], b: &[T]) -> bool {
		a == b
	}
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaseInsensitive;
impl Matcher<u8> for CaseInsensitive {
	fn is_equal(&self, a: &[u8], b: &[u8]) -> bool {
		a.eq_ignore_ascii_case(b)
	}
}

#[test]
fn empty_pattern() {
	assert_eq!(matches([], ExactMatch, b"oneshor"), None);
	assert_eq!(suffix_matches([], ExactMatch, b"oneshor"), Some(b"oneshor".as_ref()));
}

#[test]
fn exact_match() {
	{
		let pattern = [b"one".as_ref()];
		assert_eq!(matches(pattern, ExactMatch, b"oneshor "), Some(b"shor ".as_ref()));
		assert_eq!(matches(pattern, ExactMatch, b"onetour"), Some(b"tour".as_ref()));
		assert_eq!(matches(pattern, ExactMatch, b"one"), Some(b"".as_ref()));
	}
	{
		let pattern = [b"no".as_ref(), b"ze"];
		assert_eq!(matches(pattern, ExactMatch, b"noize"), Some(b"".as_ref()));
		assert_eq!(matches(pattern, ExactMatch, b"noze"), Some(b"".as_ref()));
		assert_eq!(matches(pattern, ExactMatch, b" noize"), None);
		assert_eq!(matches(pattern, ExactMatch, b"no"), None);
		assert_eq!(matches(pattern, ExactMatch, b"ze"), None);
		assert_eq!(matches(pattern, ExactMatch, b"noize "), Some(b" ".as_ref()));
	}
}

#[test]
fn case_insensitive_match() {
	let pattern = ["NOIZE".as_ref()];
	assert_eq!(matches(pattern, CaseInsensitive, b"Noize "), Some(b" ".as_ref()));
	assert_eq!(matches(pattern, CaseInsensitive, b"noIZE"), Some(b"".as_ref()));
	let pattern = [".nav".as_ref()];
	assert_eq!(suffix_matches(pattern, CaseInsensitive, b"cp_dustbowl.nav"), Some(b"".as_ref()));
	assert_eq!(suffix_matches(pattern, CaseInsensitive, b"DM_FLOOD.NAV"), Some(b"".as_ref()));
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathMatch;
impl Matcher<u8> for PathMatch {
	fn is_equal(&self, a: &[u8], b: &[u8]) -> bool {
		if a.len() != b.len() { return false }
		for (a, b) in a.iter().zip(b) {
			match (a, b) {
				(a, b) if a.eq_ignore_ascii_case(b) => {}
				(b'/', b'\\') | (b'\\', b'/') => {}
				_ => return false,
			}
		}
		true
	}
}

#[test]
fn path_match() {
	let pattern = [b"maps/".as_ref(), b".nav"];
	assert_eq!(matches(pattern, PathMatch, b"maps/"), None);
	assert_eq!(matches(pattern, PathMatch, b"Maps/DM_FLOOD.NAV"), Some(b"".as_ref()));
	assert_eq!(matches(pattern, PathMatch, b"maps/cp_dustbowl.nav"), Some(b"".as_ref()));
	assert_eq!(matches(pattern, PathMatch, b"maps/cp_dustbowl.bsp"), None);
}
