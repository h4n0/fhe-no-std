//! Secret keys for the BFV encryption scheme

use crate::{
	ciphertext::Ciphertext,
	parameters::BfvParameters,
	plaintext::{encode_pt, Plaintext},
	traits::{Decryptor, Encryptor},
};
use itertools::Itertools;
use math::{
	rq::{traits::TryConvertFrom, Poly, Representation},
	zq::Modulus,
};
use rand::{thread_rng, Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::rc::Rc;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[cfg(test)]
use crate::Encoding;
#[cfg(test)]
use num_bigint::BigUint;

/// Secret key for the BFV encryption scheme.
#[derive(Debug, PartialEq, Eq)]
pub struct SecretKey {
	pub(crate) par: Rc<BfvParameters>,
	pub(crate) s: Poly,
}

impl Zeroize for SecretKey {
	fn zeroize(&mut self) {
		self.s.zeroize();
	}
}

impl ZeroizeOnDrop for SecretKey {}

impl SecretKey {
	/// Generate a random [`SecretKey`].
	pub fn random(par: &Rc<BfvParameters>) -> Self {
		let s = Poly::small(&par.ctx, Representation::NttShoup, par.variance).unwrap();
		Self {
			par: par.clone(),
			s,
		}
	}

	/// # Safety
	///
	/// Measure the noise in a [`Ciphertext`].
	/// This operations may run in a variable time depending on the value of the noise.
	#[cfg(test)]
	pub(crate) unsafe fn measure_noise(
		&self,
		ct: &Ciphertext,
		encoding: Encoding,
	) -> Result<usize, String> {
		let plaintext = self.decrypt(ct)?;

		let mut m = encode_pt(&self.par, &plaintext, Some(encoding))?;

		// Let's disable variable time computations
		let mut c0 = ct.c0.clone();
		let mut c1 = ct.c1.clone();
		c0.disallow_variable_time_computations();
		c1.disallow_variable_time_computations();

		let mut c1_s = &c1 * &self.s;
		let mut c = &c0 + &c1_s;
		c -= &m;
		c.change_representation(Representation::PowerBasis);

		let ciphertext_modulus = self.par.ctx.modulus();
		let mut noise = 0usize;
		for coeff in Vec::<BigUint>::from(&c) {
			noise = std::cmp::max(
				noise,
				std::cmp::min(coeff.bits(), (ciphertext_modulus - &coeff).bits()) as usize,
			)
		}

		c1_s.zeroize();
		c.zeroize();
		m.zeroize();

		Ok(noise)
	}
}

impl Encryptor for SecretKey {
	type Error = String;

	fn encrypt(&self, pt: &Plaintext) -> Result<Ciphertext, Self::Error> {
		let mut seed = <ChaCha8Rng as SeedableRng>::Seed::default();
		thread_rng().fill(&mut seed);

		let mut a = Poly::random_from_seed(&self.par.ctx, Representation::Ntt, seed);
		let mut a_s = &a * &self.s;

		let mut b = Poly::small(&self.par.ctx, Representation::Ntt, self.par.variance).unwrap();
		b -= &a_s;

		let mut m = encode_pt(&self.par, pt, None)?;
		b += &m;

		// Zeroize the temporary variables holding sensitive information.
		a_s.zeroize();
		m.zeroize();

		// It is now safe to enable variable time computations.
		unsafe {
			a.allow_variable_time_computations();
			b.allow_variable_time_computations()
		}

		Ok(Ciphertext {
			par: self.par.clone(),
			seed: Some(seed),
			c0: b,
			c1: a,
		})
	}
}

impl Decryptor for SecretKey {
	type Error = String;

	fn decrypt(&self, ct: &Ciphertext) -> Result<Plaintext, Self::Error> {
		if self.par != ct.par {
			Err("Incompatible BFV parameters".to_string())
		} else {
			// Let's disable variable time computations
			let mut c0 = ct.c0.clone();
			let mut c1 = ct.c1.clone();
			c0.disallow_variable_time_computations();
			c1.disallow_variable_time_computations();

			let mut c1_s = &c1 * &self.s;
			let mut c = &c0 + &c1_s;
			c.change_representation(Representation::PowerBasis);
			let mut d = self.par.scaler.scale(&c, false)?;
			// TODO: Can we handle plaintext moduli that are BigUint?
			let mut v = Vec::<u64>::from(&d)
				.iter_mut()
				.map(|vi| *vi + self.par.plaintext.modulus())
				.collect_vec();
			let mut w = v[..self.par.polynomial_degree].to_vec();
			let q = Modulus::new(self.par.ciphertext_moduli[0]).unwrap();
			q.reduce_vec(&mut w);
			self.par.plaintext.reduce_vec(&mut w);

			let mut poly =
				Poly::try_convert_from(&w as &[u64], &self.par.ctx, Representation::PowerBasis)?;
			poly.change_representation(Representation::Ntt);

			let pt = Plaintext {
				par: self.par.clone(),
				value: unsafe {
					self.par
						.plaintext
						.center_vec_vt(&w[..self.par.polynomial_degree])
				},
				encoding: None,
				poly_ntt: poly,
			};

			// Zeroize the temporary variables potentially holding sensitive information.
			c1_s.zeroize();
			c.zeroize();
			d.zeroize();
			v.zeroize();

			Ok(pt)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::SecretKey;
	use crate::{
		parameters::BfvParameters,
		traits::{Decryptor, Encoder, Encryptor},
		Encoding, Plaintext,
	};
	use math::rq::Representation;
	use std::rc::Rc;

	#[test]
	fn test_keygen() {
		let params = Rc::new(BfvParameters::default(1));
		let sk = SecretKey::random(&params);
		assert_eq!(sk.par, params);

		let mut s = sk.s.clone();
		s.change_representation(Representation::PowerBasis);
		let coefficients = Vec::<u64>::from(&s);
		coefficients.iter().for_each(|ci| {
			// Check that this is a small polynomial
			assert!(
				*ci <= 2 * sk.par.variance as u64
					|| *ci >= (sk.par.ciphertext_moduli[0] - 2 * sk.par.variance as u64)
			)
		})
	}

	#[test]
	fn test_encrypt_decrypt() -> Result<(), String> {
		for params in [
			Rc::new(BfvParameters::default(1)),
			Rc::new(BfvParameters::default(2)),
		] {
			for _ in 0..100 {
				let sk = SecretKey::random(&params);

				let pt = Plaintext::try_encode(
					&[1u64, 2, 3, 4, 5, 6, 7, 8] as &[u64],
					Encoding::Poly,
					&params,
				)?;
				let ct = sk.encrypt(&pt)?;
				let pt2 = sk.decrypt(&ct);

				println!("Noise: {}", unsafe {
					sk.measure_noise(&ct, Encoding::Poly)?
				});
				assert!(pt2.is_ok_and(|pt2| pt2 == &pt));
			}
		}

		Ok(())
	}
}
