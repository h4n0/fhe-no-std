//! Ciphertext type in the BFV encryption scheme.

use crate::{parameters::BfvParameters, RelinearizationKey};
use itertools::izip;
use math::rq::{
	traits::{ContextSwitcher, TryConvertFrom},
	Poly, Representation,
};
use ndarray::{Array2, Axis};
use num_bigint::BigUint;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::{
	ops::{Add, AddAssign, Neg, Sub, SubAssign},
	rc::Rc,
};

/// A ciphertext encrypting a plaintext.
#[derive(Debug, Clone, PartialEq)]
pub struct Ciphertext {
	/// The parameters of the underlying BFV encryption scheme.
	pub(crate) par: Rc<BfvParameters>,

	/// The seed that generated the polynomial c0 in a fresh ciphertext.
	pub(crate) seed: Option<<ChaCha8Rng as SeedableRng>::Seed>,

	/// The ciphertext element c0.
	pub(crate) c0: Poly,

	/// The ciphertext element c1.
	pub(crate) c1: Poly,
}

impl Add<&Ciphertext> for &Ciphertext {
	type Output = Ciphertext;

	fn add(self, rhs: &Ciphertext) -> Ciphertext {
		debug_assert_eq!(self.par, rhs.par);

		Ciphertext {
			par: self.par.clone(),
			seed: None,
			c0: &self.c0 + &rhs.c0,
			c1: &self.c1 + &rhs.c1,
		}
	}
}

impl AddAssign<&Ciphertext> for Ciphertext {
	fn add_assign(&mut self, rhs: &Ciphertext) {
		debug_assert_eq!(self.par, rhs.par);

		self.c0 += &rhs.c0;
		self.c1 += &rhs.c1;
		self.seed = None
	}
}

impl Sub<&Ciphertext> for &Ciphertext {
	type Output = Ciphertext;

	fn sub(self, rhs: &Ciphertext) -> Ciphertext {
		assert_eq!(self.par, rhs.par);

		Ciphertext {
			par: self.par.clone(),
			seed: None,
			c0: &self.c0 - &rhs.c0,
			c1: &self.c1 - &rhs.c1,
		}
	}
}

impl SubAssign<&Ciphertext> for Ciphertext {
	fn sub_assign(&mut self, rhs: &Ciphertext) {
		debug_assert_eq!(self.par, rhs.par);

		self.c0 -= &rhs.c0;
		self.c1 -= &rhs.c1;
		self.seed = None
	}
}

impl Neg for &Ciphertext {
	type Output = Ciphertext;

	fn neg(self) -> Ciphertext {
		Ciphertext {
			par: self.par.clone(),
			seed: None,
			c0: -&self.c0,
			c1: -&self.c1,
		}
	}
}

#[allow(dead_code)]
fn print_poly(s: &str, p: &Poly) {
	println!("{} = {:?}", s, Vec::<BigUint>::from(p))
}

/// Multiply two ciphertext and relinearize.
pub fn mul(
	ct0: &Ciphertext,
	ct1: &Ciphertext,
	rk: &RelinearizationKey,
) -> Result<Ciphertext, String> {
	// Extend
	let mut now = std::time::SystemTime::now();
	let c00 = ct0.par.extender.switch_context(&ct0.c0).unwrap();
	let c01 = ct0.par.extender.switch_context(&ct0.c1).unwrap();
	let c10 = ct1.par.extender.switch_context(&ct1.c0).unwrap();
	let c11 = ct1.par.extender.switch_context(&ct1.c1).unwrap();
	println!("Extend: {:?}", now.elapsed().unwrap());

	// Multiply
	now = std::time::SystemTime::now();
	let mut c0 = &c00 * &c10;
	let mut c1 = &c00 * &c11;
	c1 += &(&c01 * &c10);
	let mut c2 = &c01 * &c11;
	c0.change_representation(Representation::PowerBasis);
	c1.change_representation(Representation::PowerBasis);
	c2.change_representation(Representation::PowerBasis);
	println!("Multiply: {:?}", now.elapsed().unwrap());

	// Scale
	// TODO: This should be faster??
	now = std::time::SystemTime::now();
	let scaler = &ct0.par.extended_scaler;
	let mut c0_scaled_coeffs =
		Array2::zeros((ct0.par.ciphertext_moduli.len(), ct0.par.polynomial_degree));
	for (mut c0_scaled_column, c0_column) in izip!(
		c0_scaled_coeffs.axis_iter_mut(Axis(1)),
		c0.coefficients().axis_iter(Axis(1))
	) {
		scaler.scale(&c0_column, &mut c0_scaled_column, false);
	}
	let mut c1_scaled_coeffs =
		Array2::zeros((ct0.par.ciphertext_moduli.len(), ct0.par.polynomial_degree));
	for (mut c1_scaled_column, c1_column) in izip!(
		c1_scaled_coeffs.axis_iter_mut(Axis(1)),
		c1.coefficients().axis_iter(Axis(1))
	) {
		scaler.scale(&c1_column, &mut c1_scaled_column, false);
	}
	let mut c2_scaled_coeffs =
		Array2::zeros((ct0.par.ciphertext_moduli.len(), ct0.par.polynomial_degree));
	for (mut c2_scaled_column, c2_column) in izip!(
		c2_scaled_coeffs.axis_iter_mut(Axis(1)),
		c2.coefficients().axis_iter(Axis(1))
	) {
		scaler.scale(&c2_column, &mut c2_scaled_column, false);
	}
	println!("Scale: {:?}", now.elapsed().unwrap());

	// Relinearize
	now = std::time::SystemTime::now();
	let mut c0 =
		Poly::try_convert_from(c0_scaled_coeffs, &ct0.par.ctx, Representation::PowerBasis)?;
	let mut c1 =
		Poly::try_convert_from(c1_scaled_coeffs, &ct0.par.ctx, Representation::PowerBasis)?;
	unsafe {
		c0.allow_variable_time_computations();
		c1.allow_variable_time_computations();
	}
	c0.change_representation(Representation::Ntt);
	c1.change_representation(Representation::Ntt);
	rk.relinearize(&mut c0, &mut c1, &c2_scaled_coeffs.view())?;
	println!("Relinearize: {:?}", now.elapsed().unwrap());

	Ok(Ciphertext {
		par: ct0.par.clone(),
		seed: None,
		c0,
		c1,
	})
}

#[cfg(test)]
mod tests {
	use super::mul;
	use crate::{
		traits::{Decoder, Decryptor, Encoder, Encryptor},
		BfvParameters, BfvParametersBuilder, Encoding, Plaintext, RelinearizationKey, SecretKey,
	};
	use proptest::collection::vec as prop_vec;
	use proptest::prelude::any;
	use std::rc::Rc;

	proptest! {
		#[test]
		fn test_add(mut a in prop_vec(any::<u64>(), 8), mut b in prop_vec(any::<u64>(), 8)) {
			for params in [Rc::new(BfvParameters::default_one_modulus()),
						   Rc::new(BfvParameters::default_two_moduli())] {
				params.plaintext.reduce_vec(&mut a);
				params.plaintext.reduce_vec(&mut b);
				let mut c = a.clone();
				params.plaintext.add_vec(&mut c, &b);

				let sk = SecretKey::random(&params);

				for encoding in [Encoding::Poly, Encoding::Simd] {
					let pt_a = Plaintext::try_encode(&a as &[u64], encoding.clone(), &params).unwrap();
					let pt_b = Plaintext::try_encode(&b as &[u64], encoding.clone(), &params).unwrap();

					let mut ct_a = sk.encrypt(&pt_a).unwrap();
					let ct_b = sk.encrypt(&pt_b).unwrap();
					let ct_c = &ct_a + &ct_b;
					ct_a += &ct_b;

					let pt_c = sk.decrypt(&ct_c).unwrap();
					assert_eq!(Vec::<u64>::try_decode(&pt_c, encoding.clone()).unwrap(), c);
					let pt_c = sk.decrypt(&ct_a).unwrap();
					assert_eq!(Vec::<u64>::try_decode(&pt_c, encoding.clone()).unwrap(), c);
				}
			}
		}

		#[test]
		fn test_sub(mut a in prop_vec(any::<u64>(), 8), mut b in prop_vec(any::<u64>(), 8)) {
			for params in [Rc::new(BfvParameters::default_one_modulus()),
						   Rc::new(BfvParameters::default_two_moduli())] {
				params.plaintext.reduce_vec(&mut a);
				params.plaintext.reduce_vec(&mut b);
				let mut c = a.clone();
				params.plaintext.sub_vec(&mut c, &b);

				let sk = SecretKey::random(&params);

				for encoding in [Encoding::Poly, Encoding::Simd] {
					let pt_a = Plaintext::try_encode(&a as &[u64], encoding.clone(), &params).unwrap();
					let pt_b = Plaintext::try_encode(&b as &[u64], encoding.clone(), &params).unwrap();

					let mut ct_a = sk.encrypt(&pt_a).unwrap();
					let ct_b = sk.encrypt(&pt_b).unwrap();
					let ct_c = &ct_a - &ct_b;
					ct_a -= &ct_b;

					let pt_c = sk.decrypt(&ct_c).unwrap();
					assert_eq!(Vec::<u64>::try_decode(&pt_c, encoding.clone()).unwrap(), c);
					let pt_c = sk.decrypt(&ct_a).unwrap();
					assert_eq!(Vec::<u64>::try_decode(&pt_c, encoding.clone()).unwrap(), c);
				}
			}
		}

		#[test]
		fn test_neg(mut a in prop_vec(any::<u64>(), 8)) {
			for params in [Rc::new(BfvParameters::default_one_modulus()),
						   Rc::new(BfvParameters::default_two_moduli())] {
				params.plaintext.reduce_vec(&mut a);
				let mut c = a.clone();
				params.plaintext.neg_vec(&mut c);

				let sk = SecretKey::random(&params);
				for encoding in [Encoding::Poly, Encoding::Simd] {
					let pt_a = Plaintext::try_encode(&a as &[u64], encoding.clone(), &params).unwrap();

					let ct_a = sk.encrypt(&pt_a).unwrap();
					let ct_c = -&ct_a;

					let pt_c = sk.decrypt(&ct_c).unwrap();
					assert_eq!(Vec::<u64>::try_decode(&pt_c, encoding.clone()).unwrap(), c);
				}
			}
		}
	}

	#[test]
	fn test_mul() -> Result<(), String> {
		let par = Rc::new(BfvParameters::default_two_moduli());
		for _ in 0..50 {
			// We will encode `values` in an Simd format, and check that the product is computed correctly.
			let values = par.plaintext.random_vec(par.polynomial_degree);
			let mut expected = values.clone();
			par.plaintext.mul_vec(&mut expected, &values);

			let sk = SecretKey::random(&par);
			let rk = RelinearizationKey::new(&sk)?;
			let pt = Plaintext::try_encode(&values as &[u64], Encoding::Simd, &par)?;

			let ct1 = sk.encrypt(&pt)?;
			let ct2 = sk.encrypt(&pt)?;
			let ct3 = mul(&ct1, &ct2, &rk)?;

			println!("Noise: {}", unsafe {
				sk.measure_noise(&ct3, Encoding::Simd)?
			});
			let pt = sk.decrypt(&ct3)?;
			assert_eq!(Vec::<u64>::try_decode(&pt, Encoding::Simd)?, expected);
		}
		Ok(())
	}

	#[test]
	fn test_seq_mul() -> Result<(), String> {
		let par = Rc::new(
			BfvParametersBuilder::default()
				.polynomial_degree(8192)
				.plaintext_modulus(65537)
				.ciphertext_moduli(vec![
					4611686018326724609,
					4611686018309947393,
					4611686018282684417,
					4611686018257518593,
					4611686018232352769,
				])
				.build()
				.unwrap(),
		);

		let values = par.plaintext.random_vec(par.polynomial_degree);
		let sk = SecretKey::random(&par);
		let rk = RelinearizationKey::new(&sk)?;
		let pt = Plaintext::try_encode(&values as &[u64], Encoding::Simd, &par)?;
		let mut ct1 = sk.encrypt(&pt)?;

		for _ in 0..5 {
			ct1 = mul(&ct1, &ct1, &rk)?;
			println!("Noise: {}", unsafe {
				sk.measure_noise(&ct1, Encoding::Simd)?
			});
		}

		// Empirically measured.
		assert!(unsafe { sk.measure_noise(&ct1, Encoding::Simd)? } <= 200);

		Ok(())
	}
}
