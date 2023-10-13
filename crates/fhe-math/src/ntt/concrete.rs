use std::slice;

use concrete_ntt::prime64::Plan;

use crate::zq::Modulus;

use super::{native, traits::NttOp};

/// Number-Theoretic Transform operator using the `concrete-ntt` crate.
#[derive(Debug, Clone)]
pub struct NttOperator(u64, usize, Option<Plan>, native::NttOperator);

impl PartialEq for NttOperator {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}

impl Eq for NttOperator {}

impl NttOp for NttOperator {
    /// Create an NTT operator given a modulus for a specific size.
    ///
    /// Aborts if the size is not a power of 2 that is >= 8 in debug mode.
    /// Returns None if the modulus does not support the NTT for this specific
    /// size.
    fn new(p: &Modulus, size: usize) -> Option<Self> {
        Some(Self(
            p.p,
            size,
            Plan::try_new(size, p.p),
            native::NttOperator::new(p, size)?,
        ))
    }

    /// Compute the forward NTT in place.
    fn forward(&self, a: &mut [u64]) {
        if let Some(op) = &self.2 {
            op.fwd(a)
        } else {
            self.3.forward(a)
        }
    }

    /// Compute the backward NTT in place.
    fn backward(&self, a: &mut [u64]) {
        if let Some(op) = &self.2 {
            op.inv(a);
            op.normalize(a)
        } else {
            self.3.backward(a)
        }
    }

    /// Compute the forward NTT in place in variable time.
    ///
    /// # Safety
    /// This function assumes that a_ptr points to at least `size` elements.
    /// This function is not constant time and its timing may reveal information
    /// about the value being reduced.
    unsafe fn forward_vt(&self, a_ptr: *mut u64) {
        let a = slice::from_raw_parts_mut(a_ptr, self.1);
        self.forward(a)
    }

    /// Compute the forward NTT in place in variable time in a lazily fashion.
    /// This means that the output coefficients may be up to 4 times the
    /// modulus.
    ///
    /// # Safety
    /// This function assumes that a_ptr points to at least `size` elements.
    /// This function is not constant time and its timing may reveal information
    /// about the value being reduced.
    unsafe fn forward_vt_lazy(&self, a_ptr: *mut u64) {
        self.forward_vt(a_ptr)
    }

    /// Compute the backward NTT in place in variable time.
    ///
    /// # Safety
    /// This function assumes that a_ptr points to at least `size` elements.
    /// This function is not constant time and its timing may reveal information
    /// about the value being reduced.
    unsafe fn backward_vt(&self, a_ptr: *mut u64) {
        let a = slice::from_raw_parts_mut(a_ptr, self.1);
        self.backward(a)
    }
}
