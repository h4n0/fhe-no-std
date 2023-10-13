use crate::zq::Modulus;

/// Operations of an Ntt operator.
pub trait NttOp {
    /// Create an NTT operator given a modulus for a specific size.
    ///
    /// Aborts if the size is not a power of 2 that is >= 8 in debug mode.
    /// Returns None if the modulus does not support the NTT for this specific
    /// size.
    fn new(p: &Modulus, size: usize) -> Option<Self>
    where
        Self: Sized;

    /// Compute the forward NTT in place.
    fn forward(&self, a: &mut [u64]);

    /// Compute the backward NTT in place.
    fn backward(&self, a: &mut [u64]);

    /// Compute the forward NTT in place in variable time.
    ///
    /// # Safety
    /// This function assumes that a_ptr points to at least `size` elements.
    /// This function is not constant time and its timing may reveal information
    /// about the value being reduced.
    unsafe fn forward_vt(&self, a_ptr: *mut u64);

    /// Compute the forward NTT in place in variable time in a lazily fashion.
    /// This means that the output coefficients may be up to 4 times the
    /// modulus.
    ///
    /// # Safety
    /// This function assumes that a_ptr points to at least `size` elements.
    /// This function is not constant time and its timing may reveal information
    /// about the value being reduced.
    unsafe fn forward_vt_lazy(&self, a_ptr: *mut u64);

    /// Compute the backward NTT in place in variable time.
    ///
    /// # Safety
    /// This function assumes that a_ptr points to at least `size` elements.
    /// This function is not constant time and its timing may reveal information
    /// about the value being reduced.
    unsafe fn backward_vt(&self, a_ptr: *mut u64);
}
