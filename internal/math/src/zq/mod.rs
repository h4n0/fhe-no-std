#![warn(missing_docs, unused_imports)]

//! Ring operations for moduli up to 62 bits.

pub mod nfl;
pub mod ntt;

use crate::errors::{Error, Result};
use itertools::{izip, Itertools};
use num_bigint::BigUint;
use num_traits::cast::ToPrimitive;
use rand::{distributions::Uniform, thread_rng, Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use util::is_prime;

/// Structure encapsulating an integer modulus up to 62 bits.
#[derive(Debug, Clone, PartialEq)]
pub struct Modulus {
	p: u64,
	barrett_hi: u64,
	barrett_lo: u64,
	leading_zeros: u32,
	supports_opt: bool,
	distribution: Uniform<u64>,
}

// We need to declare Eq manually because of the `Uniform` member.
impl Eq for Modulus {}

impl Modulus {
	/// Create a modulus from an integer of at most 62 bits.
	pub fn new(p: u64) -> Result<Self> {
		if p < 2 || (p >> 62) != 0 {
			Err(Error::InvalidModulus(p))
		} else {
			let barrett = ((BigUint::from(1u64) << 128usize) / p).to_u128().unwrap(); // 2^128 / p
			Ok(Self {
				p,
				barrett_hi: (barrett >> 64) as u64,
				barrett_lo: barrett as u64,
				leading_zeros: p.leading_zeros(),
				supports_opt: nfl::supports_opt(p),
				distribution: Uniform::from(0..p),
			})
		}
	}

	/// Returns the value of the modulus.
	pub const fn modulus(&self) -> u64 {
		self.p
	}

	/// Performs the modular addition of a and b in constant time.
	/// Aborts if a >= p or b >= p in debug mode.
	pub const fn add(&self, a: u64, b: u64) -> u64 {
		debug_assert!(a < self.p && b < self.p);
		Self::reduce1(a + b, self.p)
	}

	/// Performs the modular addition of a and b in variable time.
	/// Aborts if a >= p or b >= p in debug mode.
	///
	/// # Safety
	/// This function is not constant time and its timing may reveal information
	/// about the values being added.
	pub const unsafe fn add_vt(&self, a: u64, b: u64) -> u64 {
		debug_assert!(a < self.p && b < self.p);
		Self::reduce1_vt(a + b, self.p)
	}

	/// Performs the modular subtraction of a and b in constant time.
	/// Aborts if a >= p or b >= p in debug mode.
	pub const fn sub(&self, a: u64, b: u64) -> u64 {
		debug_assert!(a < self.p && b < self.p);
		Self::reduce1(a + self.p - b, self.p)
	}

	/// Performs the modular subtraction of a and b in constant time.
	/// Aborts if a >= p or b >= p in debug mode.
	///
	/// # Safety
	/// This function is not constant time and its timing may reveal information
	/// about the values being subtracted.
	const unsafe fn sub_vt(&self, a: u64, b: u64) -> u64 {
		debug_assert!(a < self.p && b < self.p);
		Self::reduce1_vt(a + self.p - b, self.p)
	}

	/// Performs the modular multiplication of a and b in constant time.
	/// Aborts if a >= p or b >= p in debug mode.
	pub const fn mul(&self, a: u64, b: u64) -> u64 {
		debug_assert!(a < self.p && b < self.p);
		self.reduce_u128((a as u128) * (b as u128))
	}

	/// Performs the modular multiplication of a and b in constant time.
	/// Aborts if a >= p or b >= p in debug mode.
	///
	/// # Safety
	/// This function is not constant time and its timing may reveal information
	/// about the values being multiplied.
	const unsafe fn mul_vt(&self, a: u64, b: u64) -> u64 {
		debug_assert!(a < self.p && b < self.p);
		Self::reduce1_vt(self.lazy_reduce_u128((a as u128) * (b as u128)), self.p)
	}

	/// Optimized modular multiplication of a and b in constant time.
	///
	/// Aborts if a >= p or b >= p in debug mode.
	pub const fn mul_opt(&self, a: u64, b: u64) -> u64 {
		debug_assert!(self.supports_opt);
		debug_assert!(a < self.p && b < self.p);

		self.reduce_opt_u128((a as u128) * (b as u128))
	}

	/// # Safety
	///
	/// Optimized modular multiplication of a and b in variable time.
	///
	/// Aborts if a >= p or b >= p in debug mode.
	const unsafe fn mul_opt_vt(&self, a: u64, b: u64) -> u64 {
		debug_assert!(self.supports_opt);
		debug_assert!(a < self.p && b < self.p);

		self.reduce_opt_u128_vt((a as u128) * (b as u128))
	}

	/// Modular negation in constant time.
	///
	/// Aborts if a >= p in debug mode.
	pub const fn neg(&self, a: u64) -> u64 {
		debug_assert!(a < self.p);
		Self::reduce1(self.p - a, self.p)
	}

	/// # Safety
	///
	/// Modular negation in variable time.
	///
	/// Aborts if a >= p in debug mode.
	const unsafe fn neg_vt(&self, a: u64) -> u64 {
		debug_assert!(a < self.p);
		Self::reduce1_vt(self.p - a, self.p)
	}

	/// Compute the Shoup representation of a.
	///
	/// Aborts if a >= p in debug mode.
	pub const fn shoup(&self, a: u64) -> u64 {
		debug_assert!(a < self.p);

		(((a as u128) << 64) / (self.p as u128)) as u64
	}

	/// Shoup multiplication of a and b in constant time.
	///
	/// Aborts if b >= p or b_shoup != shoup(b) in debug mode.
	pub const fn mul_shoup(&self, a: u64, b: u64, b_shoup: u64) -> u64 {
		Self::reduce1(self.lazy_mul_shoup(a, b, b_shoup), self.p)
	}

	/// # Safety
	///
	/// Shoup multiplication of a and b in variable time.
	///
	/// Aborts if b >= p or b_shoup != shoup(b) in debug mode.
	const unsafe fn mul_shoup_vt(&self, a: u64, b: u64, b_shoup: u64) -> u64 {
		Self::reduce1_vt(self.lazy_mul_shoup(a, b, b_shoup), self.p)
	}

	/// Lazy Shoup multiplication of a and b in constant time.
	/// The output is in the interval [0, 2 * p).
	///
	/// Aborts if b >= p or b_shoup != shoup(b) in debug mode.
	pub const fn lazy_mul_shoup(&self, a: u64, b: u64, b_shoup: u64) -> u64 {
		debug_assert!(b < self.p);
		debug_assert!(b_shoup == self.shoup(b));

		let q = ((a as u128) * (b_shoup as u128)) >> 64;
		let r = ((a as u128) * (b as u128) - q * (self.p as u128)) as u64;

		debug_assert!(r < 2 * self.p);

		r
	}

	/// Modular addition of vectors in place in constant time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub fn add_vec(&self, a: &mut [u64], b: &[u64]) {
		debug_assert_eq!(a.len(), b.len());

		izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.add(*ai, *bi));
	}

	/// # Safety
	///
	/// Modular addition of vectors in place in variable time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub unsafe fn add_vec_vt(&self, a: &mut [u64], b: &[u64]) {
		debug_assert_eq!(a.len(), b.len());

		izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.add_vt(*ai, *bi));
	}

	/// Modular subtraction of vectors in place in constant time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub fn sub_vec(&self, a: &mut [u64], b: &[u64]) {
		debug_assert_eq!(a.len(), b.len());

		izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.sub(*ai, *bi));
	}

	/// # Safety
	///
	/// Modular subtraction of vectors in place in variable time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub unsafe fn sub_vec_vt(&self, a: &mut [u64], b: &[u64]) {
		debug_assert_eq!(a.len(), b.len());

		izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.sub_vt(*ai, *bi));
	}

	/// Modular multiplication of vectors in place in constant time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub fn mul_vec(&self, a: &mut [u64], b: &[u64]) {
		debug_assert_eq!(a.len(), b.len());

		if self.supports_opt {
			izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.mul_opt(*ai, *bi));
		} else {
			izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.mul(*ai, *bi));
		}
	}

	/// Modular scalar multiplication of vectors in place in constant time.
	///
	/// Aborts if any of the values in a is >= p in debug mode.
	pub fn scalar_mul_vec(&self, a: &mut [u64], b: u64) {
		let b_shoup = self.shoup(b);
		a.iter_mut()
			.for_each(|ai| *ai = self.mul_shoup(*ai, b, b_shoup));
	}

	/// # Safety
	///
	/// Modular scalar multiplication of vectors in place in variable time.
	///
	/// Aborts if any of the values in a is >= p in debug mode.
	pub unsafe fn scalar_mul_vec_vt(&self, a: &mut [u64], b: u64) {
		let b_shoup = self.shoup(b);
		a.iter_mut()
			.for_each(|ai| *ai = self.mul_shoup_vt(*ai, b, b_shoup));
	}

	/// # Safety
	///
	/// Modular multiplication of vectors in place in variable time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub unsafe fn mul_vec_vt(&self, a: &mut [u64], b: &[u64]) {
		debug_assert_eq!(a.len(), b.len());

		if self.supports_opt {
			izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.mul_opt_vt(*ai, *bi));
		} else {
			izip!(a.iter_mut(), b.iter()).for_each(|(ai, bi)| *ai = self.mul_vt(*ai, *bi));
		}
	}

	/// Compute the Shoup representation of a vector.
	///
	/// Aborts if any of the values of the vector is >= p in debug mode.
	pub fn shoup_vec(&self, a: &[u64]) -> Vec<u64> {
		a.iter().map(|ai| self.shoup(*ai)).collect_vec()
	}

	/// Shoup modular multiplication of vectors in place in constant time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub fn mul_shoup_vec(&self, a: &mut [u64], b: &[u64], b_shoup: &[u64]) {
		debug_assert_eq!(a.len(), b.len());
		debug_assert_eq!(a.len(), b_shoup.len());
		debug_assert_eq!(&b_shoup, &self.shoup_vec(b));

		izip!(a.iter_mut(), b.iter(), b_shoup.iter())
			.for_each(|(ai, bi, bi_shoup)| *ai = self.mul_shoup(*ai, *bi, *bi_shoup));
	}

	/// # Safety
	///
	/// Shoup modular multiplication of vectors in place in variable time.
	///
	/// Aborts if a and b differ in size, and if any of their values is >= p in
	/// debug mode.
	pub unsafe fn mul_shoup_vec_vt(&self, a: &mut [u64], b: &[u64], b_shoup: &[u64]) {
		debug_assert_eq!(a.len(), b.len());
		debug_assert_eq!(a.len(), b_shoup.len());
		debug_assert_eq!(&b_shoup, &self.shoup_vec(b));

		izip!(a.iter_mut(), b.iter(), b_shoup.iter())
			.for_each(|(ai, bi, bi_shoup)| *ai = self.mul_shoup_vt(*ai, *bi, *bi_shoup));
	}

	/// Reduce a vector in place in constant time.
	pub fn reduce_vec(&self, a: &mut [u64]) {
		a.iter_mut().for_each(|ai| *ai = self.reduce(*ai));
	}

	/// # Safety
	///
	/// Center a value modulo p as i64 in variable time.
	/// TODO: To test and to make constant time?
	const unsafe fn center_vt(&self, a: u64) -> i64 {
		debug_assert!(a < self.p);

		if a >= self.p >> 1 {
			(a as i64) - (self.p as i64)
		} else {
			a as i64
		}
	}

	/// # Safety
	///
	/// Center a vector.
	pub unsafe fn center_vec_vt(&self, a: &[u64]) -> Vec<i64> {
		a.iter().map(|ai| self.center_vt(*ai)).collect_vec()
	}

	/// # Safety
	///
	/// Reduce a vector in place in variable time.
	pub unsafe fn reduce_vec_vt(&self, a: &mut [u64]) {
		a.iter_mut().for_each(|ai| *ai = self.reduce_vt(*ai));
	}

	/// Modular reduction of a i64 in constant time.
	const fn reduce_i64(&self, a: i64) -> u64 {
		self.reduce_u128((((self.p as i128) << 64) + (a as i128)) as u128)
	}

	/// # Safety
	///
	/// Modular reduction of a i64 in variable time.
	const unsafe fn reduce_i64_vt(&self, a: i64) -> u64 {
		self.reduce_u128_vt((((self.p as i128) << 64) + (a as i128)) as u128)
	}

	/// Reduce a vector in place in constant time.
	pub fn reduce_vec_i64(&self, a: &[i64]) -> Vec<u64> {
		a.iter().map(|ai| self.reduce_i64(*ai)).collect_vec()
	}

	/// # Safety
	///
	/// Reduce a vector in place in variable time.
	pub unsafe fn reduce_vec_i64_vt(&self, a: &[i64]) -> Vec<u64> {
		a.iter().map(|ai| self.reduce_i64_vt(*ai)).collect_vec()
	}

	/// Reduce a vector in constant time.
	pub fn reduce_vec_new(&self, a: &[u64]) -> Vec<u64> {
		a.iter().map(|ai| self.reduce(*ai)).collect_vec()
	}

	/// # Safety
	///
	/// Reduce a vector in variable time.
	pub unsafe fn reduce_vec_new_vt(&self, a: &[u64]) -> Vec<u64> {
		a.iter().map(|bi| self.reduce_vt(*bi)).collect_vec()
	}

	/// Modular negation of a vector in place in constant time.
	///
	/// Aborts if any of the values in the vector is >= p in debug mode.
	pub fn neg_vec(&self, a: &mut [u64]) {
		izip!(a.iter_mut()).for_each(|ai| *ai = self.neg(*ai));
	}

	/// # Safety
	///
	/// Modular negation of a vector in place in variable time.
	///
	/// Aborts if any of the values in the vector is >= p in debug mode.
	pub unsafe fn neg_vec_vt(&self, a: &mut [u64]) {
		izip!(a.iter_mut()).for_each(|ai| *ai = self.neg_vt(*ai));
	}

	/// Modular exponentiation in variable time.
	///
	/// Aborts if a >= p or n >= p in debug mode.
	pub fn pow(&self, a: u64, n: u64) -> u64 {
		debug_assert!(a < self.p && n < self.p);

		if n == 0 {
			1
		} else if n == 1 {
			a
		} else {
			let mut r = a;
			let mut i = (62 - n.leading_zeros()) as isize;
			while i >= 0 {
				r = self.mul(r, r);
				if (n >> i) & 1 == 1 {
					r = self.mul(r, a);
				}
				i -= 1;
			}
			r
		}
	}

	/// Modular inversion in variable time.
	///
	/// Returns None if p is not prime or a = 0.
	/// Aborts if a >= p in debug mode.
	pub fn inv(&self, a: u64) -> std::option::Option<u64> {
		if !is_prime(self.p) || a == 0 {
			None
		} else {
			let r = self.pow(a, self.p - 2);
			debug_assert_eq!(self.mul(a, r), 1);
			Some(r)
		}
	}

	/// Modular reduction of a u128 in constant time.
	pub const fn reduce_u128(&self, a: u128) -> u64 {
		Self::reduce1(self.lazy_reduce_u128(a), self.p)
	}

	/// # Safety
	///
	/// Modular reduction of a u128 in variable time.
	pub const unsafe fn reduce_u128_vt(&self, a: u128) -> u64 {
		Self::reduce1_vt(self.lazy_reduce_u128(a), self.p)
	}

	/// Modular reduction of a u64 in constant time.
	pub const fn reduce(&self, a: u64) -> u64 {
		Self::reduce1(self.lazy_reduce(a), self.p)
	}

	/// # Safety
	///
	/// Modular reduction of a u64 in constant time.
	pub const unsafe fn reduce_vt(&self, a: u64) -> u64 {
		Self::reduce1_vt(self.lazy_reduce(a), self.p)
	}

	/// Optimized modular reduction of a u128 in constant time.
	// TODO: to test
	pub const fn reduce_opt_u128(&self, a: u128) -> u64 {
		debug_assert!(self.supports_opt);
		Self::reduce1(self.lazy_reduce_opt_u128(a), self.p)
	}

	/// # Safety
	///
	/// Optimized modular reduction of a u128 in constant time.
	const unsafe fn reduce_opt_u128_vt(&self, a: u128) -> u64 {
		debug_assert!(self.supports_opt);
		Self::reduce1_vt(self.lazy_reduce_opt_u128(a), self.p)
	}

	/// Optimized modular reduction of a u64 in constant time.
	pub const fn reduce_opt(&self, a: u64) -> u64 {
		Self::reduce1(self.lazy_reduce_opt(a), self.p)
	}

	/// # Safety
	///
	/// Optimized modular reduction of a u64 in constant time.
	pub const unsafe fn reduce_opt_vt(&self, a: u64) -> u64 {
		Self::reduce1_vt(self.lazy_reduce_opt(a), self.p)
	}

	/// Return x mod p in constant time.
	///
	/// Aborts if x >= 2 * p in debug mode.
	const fn reduce1(x: u64, p: u64) -> u64 {
		debug_assert!(p >> 63 == 0);
		debug_assert!(x < 2 * p);

		let (y, _) = x.overflowing_sub(p);
		let xp = x ^ p;
		let yp = y ^ p;
		let xy = xp ^ yp;
		let xxy = x ^ xy;
		let xxy = xxy >> 63;
		let (c, _) = xxy.overflowing_sub(1);
		let r = (c & y) | ((!c) & x);

		debug_assert!(r == x % p);

		r
	}

	/// # Safety
	///
	/// Return x mod p in variable time.
	///
	/// Aborts if x >= 2 * p in debug mode.
	const unsafe fn reduce1_vt(x: u64, p: u64) -> u64 {
		debug_assert!(p >> 63 == 0);
		debug_assert!(x < 2 * p);

		if x >= p {
			x - p
		} else {
			x
		}
	}

	/// Lazy modular reduction of a in constant time.
	/// The output is in the interval [0, 2 * p).
	pub const fn lazy_reduce_u128(&self, a: u128) -> u64 {
		let a_lo = a as u64;
		let a_hi = (a >> 64) as u64;
		let p_lo_lo = ((a_lo as u128) * (self.barrett_lo as u128)) >> 64;
		let p_hi_lo = (a_hi as u128) * (self.barrett_lo as u128);
		let p_lo_hi = (a_lo as u128) * (self.barrett_hi as u128);

		let q = ((p_lo_hi + p_hi_lo + p_lo_lo) >> 64) + (a_hi as u128) * (self.barrett_hi as u128);
		let r = (a - q * (self.p as u128)) as u64;

		debug_assert!((r as u128) < 2 * (self.p as u128));
		debug_assert!(r % self.p == (a % (self.p as u128)) as u64);

		r
	}

	/// Lazy modular reduction of a in constant time.
	/// The output is in the interval [0, 2 * p).
	pub const fn lazy_reduce(&self, a: u64) -> u64 {
		let p_lo_lo = ((a as u128) * (self.barrett_lo as u128)) >> 64;
		let p_lo_hi = (a as u128) * (self.barrett_hi as u128);

		let q = (p_lo_hi + p_lo_lo) >> 64;
		let r = (a as u128 - q * (self.p as u128)) as u64;

		debug_assert!((r as u128) < 2 * (self.p as u128));
		debug_assert!(r % self.p == a % self.p);

		r
	}

	/// Lazy optimized modular reduction of a in constant time.
	/// The output is in the interval [0, 2 * p).
	///
	/// Aborts if the input is >= 2 * p in debug mode.
	pub const fn lazy_reduce_opt_u128(&self, a: u128) -> u64 {
		debug_assert!(a < (self.p as u128) * (self.p as u128));

		let q = (((self.barrett_lo as u128) * (a >> 64)) + (a << self.leading_zeros)) >> 64;
		let r = (a - q * (self.p as u128)) as u64;

		debug_assert!((r as u128) < 2 * (self.p as u128));
		debug_assert!(r % self.p == (a % (self.p as u128)) as u64);

		r
	}

	/// Lazy optimized modular reduction of a in constant time.
	/// The output is in the interval [0, 2 * p).
	///
	/// Aborts if the input is >= 2 * p in debug mode.
	const fn lazy_reduce_opt(&self, a: u64) -> u64 {
		let q = a >> (64 - self.leading_zeros);
		let r = ((a as u128) - (q as u128) * (self.p as u128)) as u64;

		debug_assert!((r as u128) < 2 * (self.p as u128));
		debug_assert!(r % self.p == a % self.p);

		r
	}

	/// Returns a random vector and a seed that generated that random element.
	///
	/// Panics if the size is 0.
	pub fn random_vec(&self, size: usize) -> Vec<u64> {
		let mut seed = <ChaCha8Rng as SeedableRng>::Seed::default();
		thread_rng().fill(&mut seed);
		self.random_vec_from_seed(size, seed)
	}

	/// Returns a random vector generated deterministically from a seed.
	///
	/// Panics if the size is 0.
	pub fn random_vec_from_seed(
		&self,
		size: usize,
		seed: <ChaCha8Rng as SeedableRng>::Seed,
	) -> Vec<u64> {
		assert_ne!(size, 0);
		let rng = rand_chacha::ChaCha8Rng::from_seed(seed);
		rng.sample_iter(self.distribution).take(size).collect_vec()
	}

	/// Length of the serialization of a vector of size `size`.
	///
	/// Panics if the size is not a multiple of 8.
	pub const fn serialization_length(&self, size: usize) -> usize {
		assert!(size % 8 == 0);
		let p_nbits = 64 - (self.p - 1).leading_zeros() as usize;
		p_nbits * size / 8
	}

	/// Serialize a vector of elements of length a multiple of 8.
	///
	/// Panics if the length of the vector is not a multiple of 8.
	pub fn serialize_vec(&self, a: &[u64]) -> Vec<u8> {
		let p_nbits = 64 - (self.p - 1).leading_zeros() as usize;
		util::transcode_forward(a, p_nbits)
	}

	/// Deserialize a vector of bytes into a vector of elements mod p.
	pub fn deserialize_vec(&self, b: &[u8]) -> Vec<u64> {
		let p_nbits = 64 - (self.p - 1).leading_zeros() as usize;
		util::transcode_backward(b, p_nbits)
	}
}

#[cfg(test)]
mod tests {
	use super::{nfl, Modulus};
	use itertools::{izip, Itertools};
	use proptest::collection::vec as prop_vec;
	use proptest::prelude::{any, BoxedStrategy, Just, Strategy};
	use rand::{thread_rng, Rng, RngCore, SeedableRng};
	use rand_chacha::ChaCha8Rng;
	use util::catch_unwind;

	// Utility functions for the proptests.

	fn valid_moduli() -> impl Strategy<Value = Modulus> {
		any::<u64>().prop_filter_map("filter invalid moduli", |p| Modulus::new(p).ok())
	}

	fn vecs() -> BoxedStrategy<(Vec<u64>, Vec<u64>)> {
		prop_vec(any::<u64>(), 1..100)
			.prop_flat_map(|vec| {
				let len = vec.len();
				(Just(vec), prop_vec(any::<u64>(), len))
			})
			.boxed()
	}

	proptest! {
		#[test]
		fn test_constructor(p: u64) {
			// 63 and 64-bit integers do not work.
			prop_assert!(Modulus::new(p | (1u64 << 62)).is_err());
			prop_assert!(Modulus::new(p | (1u64 << 63)).is_err());

			// p = 0 & 1 do not work.
			prop_assert!(Modulus::new(0u64).is_err());
			prop_assert!(Modulus::new(1u64).is_err());

			// Otherwise, all moduli should work.
			prop_assume!(p >> 2 >= 2);
			prop_assert!(Modulus::new(p >> 2).is_ok_and(|q| q.modulus() == p >> 2));
		}

		#[test]
		fn test_neg(p in valid_moduli(), mut a: u64,  mut q: u64) {
			a = p.reduce(a);
			prop_assert_eq!(p.neg(a), (p.modulus() - a) % p.modulus());
			unsafe { prop_assert_eq!(p.neg_vt(a), (p.modulus() - a) % p.modulus()) }

			q = (q % (u64::MAX - p.modulus())) + 1 + p.modulus(); // q > p
			prop_assert!(catch_unwind(|| p.neg(q)).is_err());
		}

		#[test]
		fn test_add(p in valid_moduli(), mut a: u64, mut b: u64, mut q: u64) {
			a = p.reduce(a);
			b = p.reduce(b);
			prop_assert_eq!(p.add(a, b), (a + b) % p.modulus());
			unsafe { prop_assert_eq!(p.add_vt(a, b), (a + b) % p.modulus()) }

			q = (q % (u64::MAX - p.modulus())) + 1 + p.modulus(); // q > p
			prop_assert!(catch_unwind(|| p.add(q, a)).is_err());
			prop_assert!(catch_unwind(|| p.add(a, q)).is_err());
		}

		#[test]
		fn test_sub(p in valid_moduli(), mut a: u64, mut b: u64, mut q: u64) {
			a = p.reduce(a);
			b = p.reduce(b);
			prop_assert_eq!(p.sub(a, b), (a + p.modulus() - b) % p.modulus());
			unsafe { prop_assert_eq!(p.sub_vt(a, b), (a + p.modulus() - b) % p.modulus()) }

			q = (q % (u64::MAX - p.modulus())) + 1 + p.modulus(); // q > p
			prop_assert!(catch_unwind(|| p.sub(q, a)).is_err());
			prop_assert!(catch_unwind(|| p.sub(a, q)).is_err());
		}

		#[test]
		fn test_mul(p in valid_moduli(), mut a: u64, mut b: u64, mut q: u64) {
			a = p.reduce(a);
			b = p.reduce(b);
			prop_assert_eq!(p.mul(a, b) as u128, ((a as u128) * (b as u128)) % (p.modulus() as u128));
			unsafe { prop_assert_eq!(p.mul_vt(a, b) as u128, ((a as u128) * (b as u128)) % (p.modulus() as u128)) }

			q = (q % (u64::MAX - p.modulus())) + 1 + p.modulus(); // q > p
			prop_assert!(catch_unwind(|| p.mul(q, a)).is_err());
			prop_assert!(catch_unwind(|| p.mul(a, q)).is_err());
		}

		#[test]
		fn test_mul_shoup(p in valid_moduli(), mut a: u64, mut b: u64, mut q: u64) {
			a = p.reduce(a);
			b = p.reduce(b);
			let b_shoup = p.shoup(b);
			prop_assert_eq!(p.mul_shoup(a, b, b_shoup) as u128, ((a as u128) * (b as u128)) % (p.modulus() as u128));
			unsafe { prop_assert_eq!(p.mul_shoup_vt(a, b, b_shoup) as u128, ((a as u128) * (b as u128)) % (p.modulus() as u128)) }

			q = (q % (u64::MAX - p.modulus())) + 1 + p.modulus(); // q > p
			prop_assert!(catch_unwind(|| p.shoup(q)).is_err());
			prop_assert!(catch_unwind(|| p.mul_shoup(a, q, b_shoup)).is_err());

			prop_assume!(a != b);
			prop_assert!(catch_unwind(|| p.mul_shoup(a, a, b_shoup)).is_err());
		}

		#[test]
		fn test_reduce(p in valid_moduli(), a: u64) {
			prop_assert_eq!(p.reduce(a), a % p.modulus());
			unsafe { prop_assert_eq!(p.reduce_vt(a), a % p.modulus()) }
		}

		#[test]
		fn test_reduce_i64(p in valid_moduli(), a: i64) {
			let b = if a < 0 { p.neg(p.reduce(-a as u64)) } else { p.reduce(a as u64) };
			prop_assert_eq!(p.reduce_i64(a), b);
			unsafe { prop_assert_eq!(p.reduce_i64_vt(a), b) }
		}

		#[test]
		fn test_reduce_u128(p in valid_moduli(), a: u128) {
			prop_assert_eq!(p.reduce_u128(a) as u128, a % (p.modulus() as u128));
			unsafe { prop_assert_eq!(p.reduce_u128_vt(a) as u128, a % (p.modulus() as u128)) }
		}

		#[test]
		fn test_add_vec(p in valid_moduli(), (mut a, mut b) in vecs()) {
			p.reduce_vec(&mut a);
			p.reduce_vec(&mut b);
			let c = a.clone();
			p.add_vec(&mut a, &b);
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.add(*bi, *ci)).collect_vec());
			a = c.clone();
			unsafe { p.add_vec_vt(&mut a, &b) }
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.add(*bi, *ci)).collect_vec());
		}

		#[test]
		fn test_sub_vec(p in valid_moduli(), (mut a, mut b) in vecs()) {
			p.reduce_vec(&mut a);
			p.reduce_vec(&mut b);
			let c = a.clone();
			p.sub_vec(&mut a, &b);
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.sub(*ci, *bi)).collect_vec());
			a = c.clone();
			unsafe { p.sub_vec_vt(&mut a, &b) }
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.sub(*ci, *bi)).collect_vec());
		}

		#[test]
		fn test_mul_vec(p in valid_moduli(), (mut a, mut b) in vecs()) {
			p.reduce_vec(&mut a);
			p.reduce_vec(&mut b);
			let c = a.clone();
			p.mul_vec(&mut a, &b);
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.mul(*ci, *bi)).collect_vec());
			a = c.clone();
			unsafe { p.mul_vec_vt(&mut a, &b); }
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.mul(*ci, *bi)).collect_vec());
		}

		#[test]
		fn test_scalar_mul_vec(p in valid_moduli(), mut a: Vec<u64>, mut b: u64) {
			p.reduce_vec(&mut a);
			b = p.reduce(b);
			let c = a.clone();
			p.scalar_mul_vec(&mut a, b);
			prop_assert_eq!(a, c.iter().map(|ci| p.mul(*ci, b)).collect_vec());
			a = c.clone();
			unsafe { p.scalar_mul_vec_vt(&mut a, b) }
			prop_assert_eq!(a, c.iter().map(|ci| p.mul(*ci, b)).collect_vec());
		}

		#[test]
		fn test_mul_shoup_vec(p in valid_moduli(), (mut a, mut b) in vecs()) {
			p.reduce_vec(&mut a);
			p.reduce_vec(&mut b);
			let b_shoup = p.shoup_vec(&b);
			let c = a.clone();
			p.mul_shoup_vec(&mut a, &b, &b_shoup);
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.mul(*ci, *bi)).collect_vec());
			a = c.clone();
			unsafe { p.mul_shoup_vec_vt(&mut a, &b, &b_shoup) }
			prop_assert_eq!(a, izip!(b.iter(), c.iter()).map(|(bi, ci)| p.mul(*ci, *bi)).collect_vec());
		}

		#[test]
		fn test_reduce_vec(p in valid_moduli(), a: Vec<u64>) {
			let mut b = a.clone();
			p.reduce_vec(&mut b);
			prop_assert_eq!(b, a.iter().map(|ai| p.reduce(*ai)).collect_vec());
			b = a.clone();
			unsafe { p.reduce_vec_vt(&mut b) }
			prop_assert_eq!(b, a.iter().map(|ai| p.reduce(*ai)).collect_vec());
		}

		#[test]
		fn test_reduce_vec_i64(p in valid_moduli(), a: Vec<i64>) {
			let b = p.reduce_vec_i64(&a);
			prop_assert_eq!(b, a.iter().map(|ai| p.reduce_i64(*ai)).collect_vec());
			let b = unsafe { p.reduce_vec_i64_vt(&a) };
			prop_assert_eq!(b, a.iter().map(|ai| p.reduce_i64(*ai)).collect_vec());
		}

		#[test]
		fn test_neg_vec(p in valid_moduli(), mut a: Vec<u64>) {
			p.reduce_vec(&mut a);
			let mut b = a.clone();
			p.neg_vec(&mut b);
			prop_assert_eq!(b, a.iter().map(|ai| p.neg(*ai)).collect_vec());
			b = a.clone();
			unsafe { p.neg_vec_vt(&mut b); }
			prop_assert_eq!(b, a.iter().map(|ai| p.neg(*ai)).collect_vec());
		}

		#[test]
		fn test_random_vec(p in valid_moduli(), size in 1..1000usize) {
			let v = p.random_vec(size);
			prop_assert_eq!(v.len(), size);

			let mut seed = <ChaCha8Rng as SeedableRng>::Seed::default();
			thread_rng().fill(&mut seed);
			let x = p.random_vec_from_seed(size, seed.clone());
			let y = p.random_vec_from_seed(size, seed.clone());
			prop_assert_eq!(x.len(), size);
			prop_assert_eq!(&x, &y);

			thread_rng().fill(&mut seed);
			let y = p.random_vec_from_seed(size, seed.clone());
			prop_assert_eq!(y.len(), size);
			if p.modulus().leading_zeros() <= 30 {
				prop_assert_ne!(x, y); // This will hold with probability at least 2^(-30)
			}

			prop_assert!(catch_unwind(|| p.random_vec(0)).is_err());
			prop_assert!(catch_unwind(|| p.random_vec_from_seed(0, seed)).is_err());
		}

		#[test]
		fn test_serialize(p in valid_moduli(), mut a in prop_vec(any::<u64>(), 8)) {
			p.reduce_vec(&mut a);
			let b = p.serialize_vec(&a);
			let c = p.deserialize_vec(&b);
			prop_assert_eq!(a, c);
		}
	}

	// TODO: Make a proptest.
	#[test]
	fn test_mul_opt() {
		let ntests = 100;
		let mut rng = rand::thread_rng();

		for p in [4611686018326724609] {
			let q = Modulus::new(p).unwrap();
			assert!(nfl::supports_opt(p));

			assert_eq!(q.mul_opt(0, 1), 0);
			assert_eq!(q.mul_opt(1, 1), 1);
			assert_eq!(q.mul_opt(2 % p, 3 % p), 6 % p);
			assert_eq!(q.mul_opt(p - 1, 1), p - 1);
			assert_eq!(q.mul_opt(p - 1, 2 % p), p - 2);

			assert!(catch_unwind(|| q.mul_opt(p, 1)).is_err());
			assert!(catch_unwind(|| q.mul_opt(p << 1, 1)).is_err());
			assert!(catch_unwind(|| q.mul_opt(0, p)).is_err());
			assert!(catch_unwind(|| q.mul_opt(0, p << 1)).is_err());

			for _ in 0..ntests {
				let a = rng.next_u64() % p;
				let b = rng.next_u64() % p;
				assert_eq!(
					q.mul_opt(a, b),
					(((a as u128) * (b as u128)) % (p as u128)) as u64
				);
			}
		}
	}

	// TODO: Make a proptest.
	#[test]
	fn test_pow() {
		let ntests = 10;
		let mut rng = rand::thread_rng();

		for p in [2u64, 3, 17, 1987, 4611686018326724609] {
			let q = Modulus::new(p).unwrap();

			assert_eq!(q.pow(p - 1, 0), 1);
			assert_eq!(q.pow(p - 1, 1), p - 1);
			assert_eq!(q.pow(p - 1, 2 % p), 1);
			assert_eq!(q.pow(1, p - 2), 1);
			assert_eq!(q.pow(1, p - 1), 1);

			assert!(catch_unwind(|| q.pow(p, 1)).is_err());
			assert!(catch_unwind(|| q.pow(p << 1, 1)).is_err());
			assert!(catch_unwind(|| q.pow(0, p)).is_err());
			assert!(catch_unwind(|| q.pow(0, p << 1)).is_err());

			for _ in 0..ntests {
				let a = rng.next_u64() % p;
				let b = (rng.next_u64() % p) % 1000;
				let mut c = b;
				let mut r = 1;
				while c > 0 {
					r = q.mul(r, a);
					c -= 1;
				}
				assert_eq!(q.pow(a, b), r);
			}
		}
	}

	// TODO: Make a proptest.
	#[test]
	fn test_inv() {
		let ntests = 100;
		let mut rng = rand::thread_rng();

		for p in [2u64, 3, 17, 1987, 4611686018326724609] {
			let q = Modulus::new(p).unwrap();

			assert!(q.inv(0).is_none());
			assert_eq!(q.inv(1).unwrap(), 1);
			assert_eq!(q.inv(p - 1).unwrap(), p - 1);

			assert!(catch_unwind(|| q.inv(p)).is_err());
			assert!(catch_unwind(|| q.inv(p << 1)).is_err());

			for _ in 0..ntests {
				let a = rng.next_u64() % p;
				let b = q.inv(a);

				if a == 0 {
					assert!(b.is_none())
				} else {
					assert!(b.is_some());
					assert_eq!(q.mul(a, b.unwrap()), 1)
				}
			}
		}
	}
}
