#![crate_name = "fhe"]
#![crate_type = "lib"]
#![warn(missing_docs, unused_imports)]
#![doc = include_str!("../README.md")]

mod errors;

pub mod bfv;
pub mod proto;
pub use errors::{Error, ParametersError, Result};

// Test the source code included in the README.
#[macro_use]
extern crate doc_comment;
doctest!("../README.md");

#[cfg(test)]
mod tests {
    use std::error::Error;

    use fhe_traits::{FheDecoder, FheDecrypter, FheEncoder, FheEncrypter};
    use rand::thread_rng;
    use rand_chacha::rand_core::OsRng;

    use crate::bfv::{BfvParametersBuilder, Ciphertext, Encoding, Plaintext, PublicKey, SecretKey};

    #[test]
    fn bug() -> Result<(), Box<dyn Error>> {
        let parameters = BfvParametersBuilder::new()
            .set_degree(2048)
            .set_moduli(&[0x3fffffff000001])
            .set_plaintext_modulus(1 << 10)
            .build_arc()?;
        let mut rng = thread_rng();

        let secret_key = SecretKey::random(&parameters, &mut OsRng);
        let public_key = PublicKey::new(&secret_key, &mut rng);

        let plaintext_1 = Plaintext::try_encode(&[20_u64], Encoding::poly(), &parameters)?;
        let plaintext_2 = Plaintext::try_encode(&[-7_i64], Encoding::poly(), &parameters)?;

        let ciphertext_1: Ciphertext = secret_key.try_encrypt(&plaintext_1, &mut rng)?;
        let ciphertext_2: Ciphertext = public_key.try_encrypt(&plaintext_2, &mut rng)?;

        let result = &ciphertext_1 * &ciphertext_2;

        let decrypted_plaintext = secret_key.try_decrypt(&result)?;
        let decrypted_vector = Vec::<i64>::try_decode(&decrypted_plaintext, Encoding::poly())?;

        assert_eq!(decrypted_vector[0], -140);

        Ok(())
    }
}
