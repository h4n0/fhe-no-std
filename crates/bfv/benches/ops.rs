#![feature(int_log)]

use bfv::{
	dot_product_scalar, mul, mul2,
	traits::{Encoder, Encryptor},
	BfvParameters, BfvParametersBuilder, Encoding, EvaluationKeyBuilder, Plaintext, SecretKey,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use itertools::{izip, Itertools};
use std::sync::Arc;
use std::time::Duration;

fn params() -> Result<Vec<Arc<BfvParameters>>, String> {
	let par_small = BfvParametersBuilder::new()
		.set_degree(4096)?
		.set_plaintext_modulus(1153)?
		.set_ciphertext_moduli_sizes(&[36, 37, 37])?
		.build()?;
	let par_large = BfvParametersBuilder::new()
		.set_degree(16384)?
		.set_plaintext_modulus(1153)?
		.set_ciphertext_moduli_sizes(&[62; 7])?
		.build()
		.unwrap();
	Ok(vec![Arc::new(par_small), Arc::new(par_large)])
}

pub fn ops_benchmark(c: &mut Criterion) {
	let mut group = c.benchmark_group("ops");
	group.sample_size(10);
	group.warm_up_time(Duration::from_secs(1));
	group.measurement_time(Duration::from_secs(1));

	for par in params().unwrap() {
		let sk = SecretKey::random(&par);
		let ek = EvaluationKeyBuilder::new(&sk)
			.enable_inner_sum()
			.unwrap()
			.enable_relinearization()
			.unwrap()
			.enable_column_rotation(1)
			.unwrap()
			.enable_expansion(par.degree().ilog2() as usize)
			.unwrap()
			.build()
			.unwrap();

		let pt1 = Plaintext::try_encode(&(1..16u64).collect_vec() as &[u64], Encoding::Poly, &par)
			.unwrap();
		let pt2 = Plaintext::try_encode(&(3..39u64).collect_vec() as &[u64], Encoding::Poly, &par)
			.unwrap();
		let mut c1 = sk.encrypt(&pt1).unwrap();
		let c2 = sk.encrypt(&pt2).unwrap();

		let ct_vec = (0..128)
			.map(|i| {
				let pt = Plaintext::try_encode(
					&(i..16u64).collect_vec() as &[u64],
					Encoding::Poly,
					&par,
				)
				.unwrap();
				sk.encrypt(&pt).unwrap()
			})
			.collect_vec();
		let pt_vec = (0..128)
			.map(|i| {
				Plaintext::try_encode(&(i..39u64).collect_vec() as &[u64], Encoding::Poly, &par)
					.unwrap()
			})
			.collect_vec();

		group.bench_function(
			BenchmarkId::new(
				"add",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 = &c1 + &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"add_assign",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 += &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"sub",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 = &c1 - &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"sub_assign",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 -= &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"neg",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 = -&c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"dot_product/128/naive",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| izip!(&ct_vec, &pt_vec).for_each(|(cti, pti)| c1 += cti * pti));
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"dot_product/128/opt",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| dot_product_scalar(ct_vec.iter(), pt_vec.iter()));
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"relinearize",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| ek.relinearizes(&c1));
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"rotate",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 = ek.rotates_column_by(&c1, 1).unwrap());
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"inner_sum",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| c1 = ek.computes_inner_sum(&c1).unwrap());
			},
		);

		for i in 1..par.degree().ilog2() + 1 {
			if par.degree() > 2048 && i > 4 {
				continue; // Skip slow benchmarks
			}
			group.bench_function(
				BenchmarkId::new(
					format!("expand_{}", i),
					format!(
						"{}/{}",
						par.degree(),
						par.moduli_sizes().iter().sum::<usize>()
					),
				),
				|b| {
					b.iter(|| ek.expands(&c1, i as usize).unwrap());
				},
			);
		}

		group.bench_function(
			BenchmarkId::new(
				"mul",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| &c1 * &c2);
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"mul_relinearize",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| mul(&c1, &c2, &ek));
			},
		);

		group.bench_function(
			BenchmarkId::new(
				"mul2_relinearize",
				format!(
					"{}/{}",
					par.degree(),
					par.moduli_sizes().iter().sum::<usize>()
				),
			),
			|b| {
				b.iter(|| mul2(&c1, &c2, &ek));
			},
		);
	}

	group.finish();
}

criterion_group!(ops, ops_benchmark);
criterion_main!(ops);
