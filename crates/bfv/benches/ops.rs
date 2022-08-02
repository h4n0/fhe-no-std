use bfv::{
	traits::{Encoder, Encryptor},
	BfvParameters, BfvParametersBuilder, Encoding, Plaintext, SecretKey,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use itertools::Itertools;
use std::rc::Rc;

fn params() -> Vec<Rc<BfvParameters>> {
	let par = BfvParametersBuilder::default()
		.polynomial_degree(16384)
		.plaintext_modulus(2)
		.ciphertext_moduli(vec![
			4611686018326724609,
			4611686018309947393,
			4611686018282684417,
			4611686018257518593,
			4611686018232352769,
			4611686018171535361,
			4611686018106523649,
		])
		.build()
		.unwrap();
	vec![Rc::new(par)]
}

pub fn ops_benchmark(c: &mut Criterion) {
	let mut group = c.benchmark_group("ops");

	for par in params() {
		let sk = SecretKey::random(&par);

		let pt1 = Plaintext::try_encode(&(1..16u64).collect_vec(), Encoding::Poly, &par).unwrap();
		let pt2 = Plaintext::try_encode(&(3..39u64).collect_vec(), Encoding::Poly, &par).unwrap();
		let mut c1 = sk.encrypt(&pt1).unwrap();
		let c2 = sk.encrypt(&pt2).unwrap();

		group.bench_function(
			BenchmarkId::new(
				"add",
				format!("{}/{}", par.degree(), 62 * par.moduli().len()),
			),
			|b| {
				b.iter(|| c1 += &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"sub",
				format!("{}/{}", par.degree(), 62 * par.moduli().len()),
			),
			|b| {
				b.iter(|| c1 -= &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"neg",
				format!("{}/{}", par.degree(), 62 * par.moduli().len()),
			),
			|b| {
				b.iter(|| c1 = -&c2);
			},
		);
	}

	group.finish();
}

criterion_group!(ops, ops_benchmark);
criterion_main!(ops);
