//! The encoding type for BFV.

use fhers_traits::FhePlaintextEncoding;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum EncodingEnum {
	Poly,
	Simd,
}

/// An encoding for the plaintext.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Encoding {
	pub(crate) encoding: EncodingEnum,
	pub(crate) level: usize,
}

impl Encoding {
	/// A Poly encoding encodes a vector as coefficients of a polynomial;
	/// homomorphic operations are therefore polynomial operations.
	pub fn poly() -> Self {
		Self {
			encoding: EncodingEnum::Poly,
			level: 0,
		}
	}

	/// A Simd encoding encodes a vector so that homomorphic operations are
	/// component-wise operations on the coefficients of the underlying vectors.
	/// The Simd encoding require that the plaintext modulus is congruent to 1
	/// modulo the degree of the underlying polynomial.
	pub fn simd() -> Self {
		Self {
			encoding: EncodingEnum::Simd,
			level: 0,
		}
	}

	/// A poly encoding at a given level.
	#[cfg(feature = "leveled_bfv")]
	pub fn poly_at_level(level: usize) -> Self {
		Self {
			encoding: EncodingEnum::Poly,
			level,
		}
	}

	/// A simd encoding at a given level.
	#[cfg(feature = "leveled_bfv")]
	pub fn simd_at_level(level: usize) -> Self {
		Self {
			encoding: EncodingEnum::Simd,
			level,
		}
	}
}

impl From<Encoding> for String {
	fn from(e: Encoding) -> Self {
		String::from(&e)
	}
}

impl From<&Encoding> for String {
	fn from(e: &Encoding) -> Self {
		format!("{:?}", e)
	}
}

impl FhePlaintextEncoding for Encoding {}
