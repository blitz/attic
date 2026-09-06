use super::*;

use attic::cache::CacheName;

macro_rules! cache {
    ($n:expr) => {
        CacheName::new($n.to_string()).unwrap()
    };
}

#[test]
fn test_basic() {
    /*
    $ cat json
      {
        "sub": "meow",
        "exp": 4102324986,
        "nbf": 0,
        "https://jwt.attic.rs/v1": {
          "caches": {
            "all-*": {"r":1},
            "all-ci-*": {"w":1},
            "cache-rw": {"r":1,"w":1},
            "cache-ro": {"r":1},
            "team-*": {"r":1,"w":1,"cc":1}
          }
        }
      }
    */

    #[allow(clippy::type_complexity)]
    let tokens: &[(&str, Box<dyn Fn() -> Token>)] = &[
        (
            "hs256",
            Box::new(|| {
                let secret: [u8; 18] = [
                    0xc3, 0x28, 0x20, 0x3c, 0x2d, 0x20, 0x69, 0x6e, 0x76, 0x61, 0x6c, 0x69, 0x64,
                    0x20, 0x75, 0x74, 0x66, 0x38,
                ];
                let dec_key = HS256Key::from_bytes(&secret);

                let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJleHAiOjQxMDIzMjQ5ODYsImh0dHBzOi8vand0LmF0dGljLnJzL3YxIjp7ImNhY2hlcyI6eyJhbGwtKiI6eyJyIjoxfSwiYWxsLWNpLSoiOnsidyI6MX0sImNhY2hlLXJvIjp7InIiOjF9LCJjYWNoZS1ydyI6eyJyIjoxLCJ3IjoxfSwidGVhbS0qIjp7ImNjIjoxLCJyIjoxLCJ3IjoxfX19LCJpYXQiOjE3MjgyMzI5OTYsIm5iZiI6MCwic3ViIjoibWVvdyJ9.wESluTI5K5v2W1WISGwAjazKMMUZBD-zSUYN-_XFN9I";

                Token::from_jwt(token, &SignatureType::HS256(dec_key), &None, &None).unwrap()
            }),
        ),
        (
            "rs256",
            Box::new(|| {
                // nix shell nixpkgs#jwt-cli
                // openssl genpkey -out rs256 -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -outform der
                // BASE64_SECRET=$(openssl rsa -in rs256 -outform PEM -traditional | base64 -w0)
                let secret = "
-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA5FkjtLG5yy/i0YgbD1yBA+mFrCf/6bCatL1ECzi4mmehRe70
AD/GRHxSUA+sJYx+Y69r/DjAk68ReCW8oaPYxmsmQ4nU3fpga7YajgvhZelkrmh/
gVTEkELmHfRmBL/9ilOm2Dsma5aPZ4HYzzYiv2opQyPdguw2Yqmo176NLvYB2jIO
/GqdtNy+sOWoz6KUIYIkHVYNG0CUpSsupjQ2zU5Y0W6QyMAaVwPN4IIOyWYCpetD
1ImlXzHQ9s85qRZyKkmbdXmMPUZe/zDqsaEwylpYiODcl7QaNPO13fy7PkP2epuI
NNmgQ4XAt2AxesJrNbmK8hmb3whEvd64E0gDWQIDAQABAoIBABDzcQwb2V/0+RB2
2xNj2YvxzON/wKaXXpSmLCPtHP8RTE6Fs4VFNrGkzPN2hl/vMv6xagG6MMmFyHUz
z/Hr2M6564g99hhYWsoEJap/xUascbXkufpe0Yyn+q8kkmIt4mNfXFZW5b482f5k
DDUtnpy5A8EhK3Npl4vxbkA9u/tNUeOSGNHOaVcpwDEXC5rxnaqNnp2C0kP884H6
RoiYVAxo+GiZMW8H8TfIulzxwsN2BuMqCf8ea4mD3JQTvvDHaPs8yRSRPwRiGaI3
urlTfv88Sm/kOh/CvJJhFxBVEMV22udMRe7/siMkemYoRxZMbcDeP+huFKIY4R0J
6tIPt7UCgYEA99a/b3xPlAhtrM6uITQsPwAXAH7CSW/QRuRTMeaauH2OlF+cfjL4
IKSlw/PiKZPI5LTV3fUfNV55l8VGO+lOebLXgipX3pjH0fkp2ct6Ji7hl4iIW+Ht
ZI4OJbF0M0DLwrJGwOnP/kkDsqIoHl/Lu0Q3aqJmQT+/pnxGO7Gmdl8CgYEA694W
dqv6qxV1yWFxAfN8Mae+iL/qcUaNo9g3/kf/9vwUwmpDoGLgiUK1fJopTbPcqh0G
mmFDCuv3T49/rSi9uN3bo6riWEIxTX5aKEJ9iHALX2FX7FH2QuFFXL3Cg4rGo/ZC
gcRLnKwfkrTVtqxGZ63xbk/pZGZ6mMm5VCCrMUcCgYEAsrTOZP0mBH/vVWPSe267
WNIfw+OjBIDzlaqdsqWtespOPP6UQQtPj3opbRo2QfSmLwOWEu3lCv6Mfrto4Vph
k685ZkpSAdf4fZdEbh8ifNXhJPr2GArYumEUImnKeAqI4mLaUdBGgfz0BaKXew9o
QCf4LZPcV8A32TxTCEgY19ECgYAM8SayZEVg1dCcuCgHP2D1KIsf3cfzZzemYdrQ
rQqydqp88G+9gS9o2Kw0phDWHqRhAS63kdan5sKvLuSGj9G5LxM6K8o3pYonAmPY
Ca3xqpQ1K5YzdVvZ15qCuDbQGPFFVeHYVPkBI8CnwBxp5ZIHZlf1AZWA2M6pS4hL
wW8jSQKBgQCBcIn58cIffHf238IBoftuQUsDFFry3iEiijSbbuZpuVo3/jVmK2hF
KlT/lhD7VtbuWza0oVBfCifj2vvKjfgIz6qwRmTl/CJ9VuCGMB5Tnypiw8Khuz+H
4/kpt7MqoVCGQJ7uYT2C65+Bj6IgRpPOOskuJ5mQgAem47x0kVtRzg==
-----END RSA PRIVATE KEY-----
";

                let dec_key = RS256KeyPair::from_pem(secret).unwrap();

                // TOKEN=$(jq -c < json | jwt encode --alg RS256 --secret @./rs256 -)
                let token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.eyJleHAiOjQxMDIzMjQ5ODYsImh0dHBzOi8vand0LmF0dGljLnJzL3YxIjp7ImNhY2hlcyI6eyJhbGwtKiI6eyJyIjoxfSwiYWxsLWNpLSoiOnsidyI6MX0sImNhY2hlLXJvIjp7InIiOjF9LCJjYWNoZS1ydyI6eyJyIjoxLCJ3IjoxfSwidGVhbS0qIjp7ImNjIjoxLCJyIjoxLCJ3IjoxfX19LCJpYXQiOjE3MjIwMDUwNzksIm5iZiI6MCwic3ViIjoibWVvdyJ9.Zs24IUbQOpOjhEe0sfsoSSJhDrzf4v-_wX_ceKqHeb2MERY8XSIQ1RPTNVeOW4LfJHumJj_rxh8Wv2BRGZSMldrTt0Ab_N7FnkhA37_jnRvgvEjSG3V4fC8aA4KoOa-43NRpg4HmPxiXte5-6LneBOR94Wss868wC1b_2yX2zCc1wQoZA3LNo-CRLnL4Yp5wY4Bbgyguv_9mfqXVYZykZnxumyGwVFD-Rub3KQ9d53Rf9tKcvRk9qxO2q8F2PKjeaUBG2xZtGwkWTMvSmwR1dKtkPUyPggOzbLoUG-6fxfo7D3NyL5qWCSN_7CkI-xlsRSLY1gTq-FqXvcpHeZbc8w";

                Token::from_jwt(token, &SignatureType::RS256(dec_key), &None, &None).unwrap()
            }),
        ),
    ];

    for (name, decode) in tokens {
        eprintln!("Testing {name}");

        // NOTE(cole-h): check that we get a consistent iteration order when getting permissions for
        // caches -- this depends on the order of the fields in the token, but should otherwise be
        // consistent between iterations
        let mut was_ever_wrong = false;
        for _ in 0..=1_000 {
            // NOTE(cole-h): we construct a new Token every iteration in order to get different "random
            // state"
            let decoded = decode();
            let perm_all_ci = decoded.get_permission_for_cache(&cache! { "all-ci-abc" });

            // NOTE(cole-h): if the iteration order of the token is inconsistent, the permissions may be
            // retrieved from the `all-ci-*` pattern (which only allows writing/pushing), even though
            // the `all-*` pattern (which only allows reading/pulling) is specified first
            if perm_all_ci.require_pull().is_err() || perm_all_ci.require_push().is_ok() {
                was_ever_wrong = true;
            }
        }
        assert!(
            !was_ever_wrong,
            "Iteration order should be consistent to prevent random auth failures (and successes)"
        );

        let decoded = decode();

        let perm_rw = decoded.get_permission_for_cache(&cache! { "cache-rw" });

        assert!(perm_rw.pull);
        assert!(perm_rw.push);
        assert!(!perm_rw.delete);
        assert!(!perm_rw.create_cache);

        assert!(perm_rw.require_pull().is_ok());
        assert!(perm_rw.require_push().is_ok());
        assert!(perm_rw.require_delete().is_err());
        assert!(perm_rw.require_create_cache().is_err());

        let perm_ro = decoded.get_permission_for_cache(&cache! { "cache-ro" });

        assert!(perm_ro.pull);
        assert!(!perm_ro.push);
        assert!(!perm_ro.delete);
        assert!(!perm_ro.create_cache);

        assert!(perm_ro.require_pull().is_ok());
        assert!(perm_ro.require_push().is_err());
        assert!(perm_ro.require_delete().is_err());
        assert!(perm_ro.require_create_cache().is_err());

        let perm_team = decoded.get_permission_for_cache(&cache! { "team-xyz" });

        assert!(perm_team.pull);
        assert!(perm_team.push);
        assert!(!perm_team.delete);
        assert!(perm_team.create_cache);

        assert!(perm_team.require_pull().is_ok());
        assert!(perm_team.require_push().is_ok());
        assert!(perm_team.require_delete().is_err());
        assert!(perm_team.require_create_cache().is_ok());

        assert!(!decoded
            .get_permission_for_cache(&cache! { "forbidden-cache" })
            .can_discover());
    }
}
