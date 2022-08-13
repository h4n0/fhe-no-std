//! Create parameters for the BFV encryption scheme

use itertools::Itertools;
use math::{
	rns::{RnsContext, ScalingFactor},
	rq::{scaler::Scaler, traits::TryConvertFrom, Context, Poly, Representation},
	zq::{nfl::generate_prime, ntt::NttOperator, Modulus},
};
use ndarray::ArrayView1;
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use std::sync::Arc;

/// Parameters for the BFV encryption scheme.
#[derive(Debug, PartialEq, Eq)]
pub struct BfvParameters {
	/// Number of coefficients in a polynomial.
	polynomial_degree: usize,

	/// Modulus of the plaintext.
	plaintext_modulus: u64,

	/// Vector of coprime moduli q_i for the ciphertext.
	/// One and only one of `ciphertext_moduli` or `ciphertext_moduli_sizes` must be specified.
	pub(crate) ciphertext_moduli: Vec<u64>,

	/// Vector of the sized of the coprime moduli q_i for the ciphertext.
	/// One and only one of `ciphertext_moduli` or `ciphertext_moduli_sizes` must be specified.
	ciphertext_moduli_sizes: Vec<usize>,

	/// Error variance
	pub(crate) variance: usize,

	/// Context for the underlying polynomials
	pub(crate) ctx: Arc<Context>,

	/// Ntt operator for the SIMD plaintext, if possible.
	pub(crate) op: Option<Arc<NttOperator>>,

	/// Scaling polynomial for the plaintext
	pub(crate) delta: Poly,
	pub(crate) delta_minimized: Poly,

	/// Q modulo the plaintext modulus
	pub(crate) q_mod_t: u64,
	pub(crate) q_mod_t_minimized: u64,

	/// Down scaler for the plaintext
	pub(crate) scaler: Scaler,
	pub(crate) scaler_minimized: Scaler,

	/// Plaintext Modulus
	// TODO: How can we handle this?
	pub(crate) plaintext: Modulus,

	pub(crate) plaintext_ctx: Arc<Context>,

	// Parameters for the multiplications
	pub(crate) mul_1_params: MultiplicationParameters, // type 1

	pub(crate) mul_2_params: MultiplicationParameters, // type 2

	pub(crate) matrix_reps_index_map: Vec<usize>,

	pub(crate) modswitch: Scaler,
}

unsafe impl Send for BfvParameters {}

impl BfvParameters {
	/// Returns the underlying polynomial degree
	pub fn degree(&self) -> usize {
		self.polynomial_degree
	}

	/// Returns a reference to the ciphertext moduli
	pub fn moduli(&self) -> &[u64] {
		&self.ciphertext_moduli
	}

	/// Returns a reference to the ciphertext moduli
	pub fn moduli_sizes(&self) -> &[usize] {
		&self.ciphertext_moduli_sizes
	}

	#[cfg(test)]
	pub fn default(num_moduli: usize) -> Self {
		BfvParametersBuilder::new()
			.set_degree(8)
			.unwrap()
			.set_plaintext_modulus(1153)
			.unwrap()
			.set_ciphertext_moduli_sizes(&vec![62usize; num_moduli])
			.unwrap()
			.build()
			.unwrap()
	}
}

/// Builder for parameters for the Bfv encryption scheme.
#[derive(Debug)]
pub struct BfvParametersBuilder {
	degree: usize,
	plaintext: u64,
	variance: usize,
	ciphertext_moduli: Vec<u64>,
	ciphertext_moduli_sizes: Vec<usize>,
}

impl BfvParametersBuilder {
	/// Creates a new instance of the builder
	#[allow(clippy::new_without_default)]
	pub fn new() -> Self {
		Self {
			degree: Default::default(),
			plaintext: Default::default(),
			variance: 1,
			ciphertext_moduli: Default::default(),
			ciphertext_moduli_sizes: Default::default(),
		}
	}

	/// Sets the polynomial degree. Returns an error if the degree is not
	/// a power of two larger or equal to 8.
	pub fn set_degree(&mut self, degree: usize) -> Result<&mut Self, String> {
		if degree < 8 || !degree.is_power_of_two() {
			Err("The degree should be a power of two larger or equal to 8".to_string())
		} else {
			self.degree = degree;
			Ok(self)
		}
	}

	/// Sets the plaintext modulus. Returns an error if the plaintext is not between
	/// 2 and 2^62 - 1.
	pub fn set_plaintext_modulus(&mut self, plaintext: u64) -> Result<&mut Self, String> {
		let _ = Modulus::new(plaintext)?;
		self.plaintext = plaintext;
		Ok(self)
	}

	/// Sets the sizes of the ciphertext moduli.
	/// Only one of `set_ciphertext_moduli_sizes` and `set_ciphertext_moduli` can be specified.
	pub fn set_ciphertext_moduli_sizes(&mut self, sizes: &[usize]) -> Result<&mut Self, String> {
		if !self.ciphertext_moduli.is_empty() {
			Err("The set of ciphertext moduli is already specified".to_string())
		} else {
			self.ciphertext_moduli_sizes = sizes.to_owned();
			Ok(self)
		}
	}

	/// Sets the ciphertext moduli to use.
	/// Only one of `set_ciphertext_moduli_sizes` and `set_ciphertext_moduli` can be specified.
	pub fn set_ciphertext_moduli(&mut self, moduli: &[u64]) -> Result<&mut Self, String> {
		if !self.ciphertext_moduli_sizes.is_empty() {
			Err("The set of ciphertext moduli sizes is already specified".to_string())
		} else {
			self.ciphertext_moduli = moduli.to_owned();
			Ok(self)
		}
	}

	/// Sets the error variance. Returns an error if the variance is not between
	/// one and sixteen.
	pub fn set_variance(&mut self, variance: usize) -> Result<&mut Self, String> {
		if !(1..=16).contains(&variance) {
			Err("The variance must be in 1..=16".to_string())
		} else {
			self.variance = variance;
			Ok(self)
		}
	}

	/// Generate ciphertext moduli with the specified sizes
	fn generate_moduli(moduli_sizes: &[usize], degree: usize) -> Result<Vec<u64>, String> {
		let mut moduli = vec![];
		for size in moduli_sizes {
			if *size > 62 || *size < 10 {
				return Err("The moduli sizes must be between 10 and 62 bits.".to_string());
			}

			let mut upper_bound = 1 << size;
			loop {
				if let Some(prime) = generate_prime(*size, 2 * degree as u64, upper_bound) {
					if !moduli.contains(&prime) {
						moduli.push(prime);
						break;
					} else {
						upper_bound = prime;
					}
				} else {
					return Err(
						"Could not generate enough ciphertext moduli to match the sizes provided"
							.to_string(),
					);
				}
			}
		}

		Ok(moduli)
	}

	/// Build a new `BfvParameters`.
	pub fn build(&self) -> Result<BfvParameters, String> {
		if self.degree == usize::default() {
			return Err("Unspecified degree".to_string());
		} else if self.plaintext == u64::default() {
			return Err("Unspecified plaintext modulus".to_string());
		} else if self.ciphertext_moduli.is_empty() && self.ciphertext_moduli_sizes.is_empty() {
			return Err("Unspecified ciphertext moduli".to_string());
		}

		let mut moduli = self.ciphertext_moduli.clone();
		if !self.ciphertext_moduli_sizes.is_empty() {
			moduli = Self::generate_moduli(&self.ciphertext_moduli_sizes, self.degree)?
		}
		let moduli_sizes = moduli
			.iter()
			.map(|m| 64 - m.leading_zeros() as usize)
			.collect_vec();

		let plaintext_modulus = Modulus::new(self.plaintext)?;
		let op = NttOperator::new(&plaintext_modulus, self.degree);

		// Compute the scaling factors for the plaintext
		let rns = RnsContext::new(&moduli)?;
		let ctx = Arc::new(Context::new(&moduli, self.degree)?);
		let plaintext_ctx = Arc::new(Context::new(&moduli[..1], self.degree)?);
		let scaler = Scaler::new(
			&ctx,
			&plaintext_ctx,
			ScalingFactor::new(&BigUint::from(plaintext_modulus.modulus()), rns.modulus()),
		)?;

		// Compute the NttShoup representation of delta = -1/t mod Q
		let mut delta_rests = vec![];
		for m in &moduli {
			let q = Modulus::new(*m)?;
			delta_rests.push(q.inv(q.neg(plaintext_modulus.modulus())).unwrap())
		}
		let delta = rns.lift(&ArrayView1::from(&delta_rests)); // -1/t mod Q
		let mut delta_poly = Poly::try_convert_from(&[delta], &ctx, Representation::PowerBasis)?;
		delta_poly.change_representation(Representation::NttShoup);
		let mut delta_minimized_poly = Poly::try_convert_from(
			&delta_rests[..1],
			&plaintext_ctx,
			Representation::PowerBasis,
		)?;
		delta_minimized_poly.change_representation(Representation::NttShoup);

		// Compute Q mod t
		let q_mod_t = (rns.modulus() % plaintext_modulus.modulus())
			.to_u64()
			.unwrap();
		let q_mod_t_minimized = moduli[0] % plaintext_modulus.modulus();

		// Create n+1 moduli of 62 bits for multiplication.
		let mut extended_basis = Vec::with_capacity(moduli.len() + 1);
		let mut upper_bound = 1 << 62;
		while extended_basis.len() != moduli.len() + 1 {
			upper_bound = generate_prime(62, 2 * self.degree as u64, upper_bound).unwrap();
			if !extended_basis.contains(&upper_bound) && !moduli.contains(&upper_bound) {
				extended_basis.push(upper_bound)
			}
		}

		// For the first multiplication, we want to extend to a context that is ~60 bits larger.
		let modulus_size = moduli_sizes.iter().sum::<usize>();
		let n_moduli = ((modulus_size + 60) + 61) / 62;
		let mut mul_1_moduli = vec![];
		mul_1_moduli.append(&mut moduli.clone());
		mul_1_moduli.append(&mut extended_basis[..n_moduli].to_vec());
		let mul_1_ctx = Arc::new(Context::new(&mul_1_moduli, self.degree)?);
		let mul_1_params = MultiplicationParameters::new(
			&ctx,
			&mul_1_ctx,
			ScalingFactor::one(),
			ScalingFactor::one(),
			ScalingFactor::new(&BigUint::from(plaintext_modulus.modulus()), ctx.modulus()),
		)?;

		// For the second multiplication, we use two moduli of roughly the same size
		let n_moduli = moduli.len();
		let mut mul_2_moduli = vec![];
		mul_2_moduli.append(&mut moduli.clone());
		mul_2_moduli.append(&mut extended_basis[..n_moduli].to_vec());
		let rns_2 = RnsContext::new(&extended_basis[..n_moduli])?;
		let mul_2_ctx = Arc::new(Context::new(&mul_2_moduli, self.degree)?);
		let mul_2_params = MultiplicationParameters::new(
			&ctx,
			&mul_2_ctx,
			ScalingFactor::one(),
			ScalingFactor::new(rns_2.modulus(), ctx.modulus()),
			ScalingFactor::new(&BigUint::from(plaintext_modulus.modulus()), rns_2.modulus()),
		)?;

		// We use the same code as SEAL
		// https://github.com/microsoft/SEAL/blob/82b07db635132e297282649e2ab5908999089ad2/native/src/seal/batchencoder.cpp
		let row_size = self.degree >> 1;
		let m = self.degree << 1;
		let gen = 3;
		let mut pos = 1;
		let mut matrix_reps_index_map = vec![0usize; self.degree];
		for i in 0..row_size {
			let index1 = (pos - 1) >> 1;
			let index2 = (m - pos - 1) >> 1;
			matrix_reps_index_map[i] = index1.reverse_bits() >> (self.degree.leading_zeros() + 1);
			matrix_reps_index_map[row_size | i] =
				index2.reverse_bits() >> (self.degree.leading_zeros() + 1);
			pos *= gen;
			pos &= m - 1;
		}

		let modswitch = Scaler::new(
			&ctx,
			&plaintext_ctx,
			ScalingFactor::new(
				&BigUint::from(1u64),
				&(ctx.modulus() / plaintext_ctx.modulus()),
			),
		)?;
		let scaler_minimized = Scaler::new(
			&plaintext_ctx,
			&plaintext_ctx,
			ScalingFactor::new(
				&BigUint::from(plaintext_modulus.modulus()),
				plaintext_ctx.modulus(),
			),
		)?;

		Ok(BfvParameters {
			polynomial_degree: self.degree,
			plaintext_modulus: self.plaintext,
			ciphertext_moduli: moduli,
			ciphertext_moduli_sizes: moduli_sizes,
			variance: self.variance,
			ctx,
			op: op.map(Arc::new),
			delta: delta_poly,
			delta_minimized: delta_minimized_poly,
			q_mod_t,
			q_mod_t_minimized,
			scaler,
			scaler_minimized,
			plaintext: plaintext_modulus,
			plaintext_ctx,
			mul_1_params,
			mul_2_params,
			matrix_reps_index_map,
			modswitch,
		})
	}
}

/// Multiplication parameters
#[derive(Debug, PartialEq, Eq, Default)]
pub(crate) struct MultiplicationParameters {
	pub(crate) extender_self: Scaler,
	pub(crate) extender_other: Scaler,
	pub(crate) down_scaler: Scaler,
	pub(crate) from: Arc<Context>,
	pub(crate) to: Arc<Context>,
}

impl MultiplicationParameters {
	fn new(
		from: &Arc<Context>,
		to: &Arc<Context>,
		up_self_factor: ScalingFactor,
		up_other_factor: ScalingFactor,
		down_factor: ScalingFactor,
	) -> Result<Self, String> {
		Ok(Self {
			extender_self: Scaler::new(from, to, up_self_factor)?,
			extender_other: Scaler::new(from, to, up_other_factor)?,
			down_scaler: Scaler::new(to, from, down_factor)?,
			from: from.clone(),
			to: to.clone(),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::{BfvParameters, BfvParametersBuilder};

	#[test]
	fn test_builder() -> Result<(), String> {
		let params = BfvParametersBuilder::new().build();
		assert!(params.is_err_and(|e| e.to_string() == "Unspecified degree"));

		assert!(BfvParametersBuilder::new().set_degree(7).is_err_and(
			|e| e.to_string() == "The degree should be a power of two larger or equal to 8"
		));

		assert!(BfvParametersBuilder::new().set_degree(1023).is_err_and(
			|e| e.to_string() == "The degree should be a power of two larger or equal to 8"
		));

		let params = BfvParametersBuilder::new().set_degree(1024)?.build();
		assert!(params.is_err_and(|e| e.to_string() == "Unspecified plaintext modulus"));

		assert!(BfvParametersBuilder::new()
			.set_degree(1024)?
			.set_plaintext_modulus(0)
			.is_err_and(|e| e.to_string() == "modulus should be between 2 and 2^62-1"));

		let params = BfvParametersBuilder::new()
			.set_degree(1024)?
			.set_plaintext_modulus(2)?
			.build();
		assert!(params.is_err_and(|e| e.to_string() == "Unspecified ciphertext moduli"));

		assert!(BfvParametersBuilder::new()
			.set_degree(1024)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli(&[])?
			.build()
			.is_err_and(|e| e.to_string() == "Unspecified ciphertext moduli"));

		assert!(BfvParametersBuilder::new()
			.set_degree(1024)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli(&[1153])?
			.set_ciphertext_moduli_sizes(&[62])
			.is_err_and(|e| e.to_string() == "The set of ciphertext moduli is already specified"));

		assert!(BfvParametersBuilder::new()
			.set_degree(8)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli(&[1])?
			.build()
			.is_err_and(|e| e.to_string() == "modulus should be between 2 and 2^62-1"));

		let params = BfvParametersBuilder::new()
			.set_degree(8)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli(&[2])?
			.build();
		assert!(params.is_err_and(|e| e.to_string() == "Impossible to construct a Ntt operator"));

		let params = BfvParametersBuilder::new()
			.set_degree(8)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli(&[1153])?
			.build();
		assert!(params.is_ok());

		let params = params.unwrap();
		assert_eq!(params.ciphertext_moduli, vec![1153]);
		assert_eq!(params.moduli(), vec![1153]);
		assert_eq!(params.plaintext_modulus, 2);
		assert_eq!(params.polynomial_degree, 8);
		assert_eq!(params.degree(), 8);
		assert_eq!(params.variance, 1);
		assert!(params.op.is_none());

		Ok(())
	}

	#[test]
	fn test_default() {
		let params = BfvParameters::default(1);
		assert_eq!(params.ciphertext_moduli.len(), 1);

		let params = BfvParameters::default(2);
		assert_eq!(params.ciphertext_moduli.len(), 2);
	}

	#[test]
	fn test_ciphertext_moduli() -> Result<(), String> {
		let params = BfvParametersBuilder::new()
			.set_degree(8)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli_sizes(&[62, 62, 62, 61, 60, 11])?
			.build();
		assert!(params.is_ok_and(|p| p.ciphertext_moduli
			== &[
				4611686018427387761,
				4611686018427387617,
				4611686018427387409,
				2305843009213693921,
				1152921504606846577,
				2017
			]));

		let params = BfvParametersBuilder::new()
			.set_degree(8)?
			.set_plaintext_modulus(2)?
			.set_ciphertext_moduli(&[
				4611686018427387761,
				4611686018427387617,
				4611686018427387409,
				2305843009213693921,
				1152921504606846577,
				2017,
			])?
			.build();
		assert!(params.is_ok_and(|p| p.ciphertext_moduli_sizes == &[62, 62, 62, 61, 60, 11]));

		Ok(())
	}
}
