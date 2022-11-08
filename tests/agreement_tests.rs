// Copyright 2015-2017 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

// Modifications copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR ISC

extern crate alloc;

use aws_lc_ring::{agreement, error, rand, test, test_file};

#[test]
fn agreement_traits() {
    use alloc::vec::Vec;
    use regex;
    use regex::Regex;

    let rng = rand::SystemRandom::new();
    let private_key =
        agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).unwrap();

    test::compile_time_assert_send::<agreement::EphemeralPrivateKey>();
    //test::compile_time_assert_sync::<agreement::EphemeralPrivateKey>();

    assert_eq!(
        format!("{:?}", &private_key),
        "EphemeralPrivateKey { algorithm: Algorithm { curve: P256 } }"
    );

    let public_key = private_key.compute_public_key().unwrap();
    let pubkey_re = Regex::new(
        "PublicKey \\{ algorithm: Algorithm \\{ curve: P256 \\}, bytes: \"[0-9a-f]+\" \\}",
    )
    .unwrap();
    let pubkey_debug = format!("{:?}", &public_key);

    assert!(
        pubkey_re.is_match(&pubkey_debug),
        "pubkey_debug: {}",
        pubkey_debug
    );

    let pubkey_clone = public_key.clone();
    assert_eq!(public_key.as_ref(), pubkey_clone.as_ref());
    assert_eq!(pubkey_debug, format!("{:?}", &pubkey_clone));

    test::compile_time_assert_clone::<agreement::PublicKey>();
    test::compile_time_assert_send::<agreement::PublicKey>();
    //test::compile_time_assert_sync::<agreement::PublicKey>();

    // Verify `PublicKey` implements `Debug`.
    //
    // TODO: Test the actual output.
    let _: &dyn core::fmt::Debug = &public_key;

    test::compile_time_assert_clone::<agreement::UnparsedPublicKey<&[u8]>>();
    test::compile_time_assert_copy::<agreement::UnparsedPublicKey<&[u8]>>();
    test::compile_time_assert_sync::<agreement::UnparsedPublicKey<&[u8]>>();

    test::compile_time_assert_clone::<agreement::UnparsedPublicKey<Vec<u8>>>();
    test::compile_time_assert_sync::<agreement::UnparsedPublicKey<Vec<u8>>>();

    let bytes = [0x01, 0x02, 0x03];

    let unparsed_public_key = agreement::UnparsedPublicKey::new(&agreement::X25519, &bytes);
    let unparsed_pubkey_clone = unparsed_public_key;
    assert_eq!(
        format!("{:?}", unparsed_public_key),
        r#"UnparsedPublicKey { algorithm: Algorithm { curve: Curve25519 }, bytes: "010203" }"#
    );
    assert_eq!(
        format!("{:?}", unparsed_pubkey_clone),
        r#"UnparsedPublicKey { algorithm: Algorithm { curve: Curve25519 }, bytes: "010203" }"#
    );

    let unparsed_public_key =
        agreement::UnparsedPublicKey::new(&agreement::X25519, Vec::from(bytes));
    let unparsed_pubkey_clone = unparsed_public_key.clone();
    assert_eq!(
        format!("{:?}", unparsed_public_key),
        r#"UnparsedPublicKey { algorithm: Algorithm { curve: Curve25519 }, bytes: "010203" }"#
    );
    assert_eq!(
        format!("{:?}", unparsed_pubkey_clone),
        r#"UnparsedPublicKey { algorithm: Algorithm { curve: Curve25519 }, bytes: "010203" }"#
    );
}

#[test]
fn agreement_agree_ephemeral() {
    let rng = rand::SystemRandom::new();

    test::run(
        test_file!("data/agreement_tests.txt"),
        |section, test_case| {
            assert_eq!(section, "");

            let curve_name = test_case.consume_string("Curve");
            let alg = alg_from_curve_name(&curve_name);
            let peer_public =
                agreement::UnparsedPublicKey::new(alg, test_case.consume_bytes("PeerQ"));

            match test_case.consume_optional_string("Error") {
                None => {
                    let my_private_bytes = test_case.consume_bytes("D");
                    let my_private = {
                        let rng = test::rand::FixedSliceRandom {
                            bytes: &my_private_bytes,
                        };
                        agreement::EphemeralPrivateKey::generate(alg, &rng)?
                    };
                    let my_public = test_case.consume_bytes("MyQ");
                    let output = test_case.consume_bytes("Output");

                    assert_eq!(my_private.algorithm(), alg);

                    let computed_public = my_private.compute_public_key().unwrap();
                    assert_eq!(computed_public.as_ref(), &my_public[..]);

                    assert_eq!(my_private.algorithm(), alg);

                    let result =
                        agreement::agree_ephemeral(my_private, &peer_public, (), |key_material| {
                            assert_eq!(key_material, &output[..]);
                            Ok(())
                        });
                    assert_eq!(
                        result,
                        Ok(()),
                        "Failed on private key: {:?}",
                        test::to_hex(my_private_bytes)
                    );
                }

                Some(_) => {
                    fn kdf_not_called(_: &[u8]) -> Result<(), ()> {
                        panic!(
                            "The KDF was called during ECDH when the peer's \
                         public key is invalid."
                        );
                    }
                    let dummy_private_key = agreement::EphemeralPrivateKey::generate(alg, &rng)?;
                    assert!(agreement::agree_ephemeral(
                        dummy_private_key,
                        &peer_public,
                        (),
                        kdf_not_called
                    )
                    .is_err());
                }
            }

            Ok(())
        },
    );
}

#[test]
fn test_agreement_ecdh_x25519_rfc_iterated() {
    fn expect_iterated_x25519(
        expected_result: &str,
        range: core::ops::Range<usize>,
        k: &mut Vec<u8>,
        u: &mut Vec<u8>,
    ) {
        for _ in range {
            let new_k = x25519(k, u);
            *u = k.clone();
            *k = new_k;
        }
        assert_eq!(&h(expected_result), k);
    }

    let mut k = h("0900000000000000000000000000000000000000000000000000000000000000");
    let mut u = k.clone();

    expect_iterated_x25519(
        "422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079",
        0..1,
        &mut k,
        &mut u,
    );
    expect_iterated_x25519(
        "684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51",
        1..1_000,
        &mut k,
        &mut u,
    );

    // The spec gives a test vector for 1,000,000 iterations but it takes
    // too long to do 1,000,000 iterations by default right now. This
    // 10,000 iteration vector is self-computed.
    expect_iterated_x25519(
        "2c125a20f639d504a7703d2e223c79a79de48c4ee8c23379aa19a62ecd211815",
        1_000..10_000,
        &mut k,
        &mut u,
    );

    if cfg!(feature = "slow_tests") {
        expect_iterated_x25519(
            "7c3911e0ab2586fd864497297e575e6f3bc601c0883c30df5f4dd2d24f665424",
            10_000..1_000_000,
            &mut k,
            &mut u,
        );
    }
}

fn x25519(private_key: &[u8], public_key: &[u8]) -> Vec<u8> {
    x25519_(private_key, public_key).unwrap()
}

fn x25519_(private_key: &[u8], public_key: &[u8]) -> Result<Vec<u8>, error::Unspecified> {
    let rng = test::rand::FixedSliceRandom { bytes: private_key };
    let private_key = agreement::EphemeralPrivateKey::generate(&agreement::X25519, &rng)?;
    let public_key = agreement::UnparsedPublicKey::new(&agreement::X25519, public_key);
    agreement::agree_ephemeral(
        private_key,
        &public_key,
        error::Unspecified,
        |agreed_value| Ok(Vec::from(agreed_value)),
    )
}

fn h(s: &str) -> Vec<u8> {
    match test::from_hex(s) {
        Ok(v) => v,
        Err(msg) => {
            panic!("{} in {}", msg, s);
        }
    }
}

fn alg_from_curve_name(curve_name: &str) -> &'static agreement::Algorithm {
    if curve_name == "P-256" {
        &agreement::ECDH_P256
    } else if curve_name == "P-384" {
        &agreement::ECDH_P384
    } else if curve_name == "X25519" {
        &agreement::X25519
    } else {
        panic!("Unsupported curve: {}", curve_name);
    }
}
