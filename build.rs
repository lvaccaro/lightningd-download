#[cfg(not(feature = "download"))]
mod download {}

#[cfg(any(not(feature = "download"), feature = "doc"))]
fn main() {}

#[cfg(all(feature = "download", not(feature = "doc")))]
fn main() {
    download::start().unwrap();
}

#[cfg(all(feature = "download", not(feature = "doc")))]
mod download {

    use anyhow::Context;
    use bitcoin_hashes::{sha256, Hash};
    use flate2::read::GzDecoder;
    use xz::read::XzDecoder;
    use std::fs::File;
    use std::io::{BufRead, BufReader, Read};
    use std::path::Path;
    use std::str::FromStr;
    use tar::Archive;

    include!("src/versions.rs");
/*
    #[cfg(all(
        target_os = "macos",
        any(target_arch = "x86_64", target_arch = "aarch64"),
    ))]
    fn download_filename() -> String {
        if cfg!(not(feature = "23_1")) {
            format!("clightning-{}-osx64.tar.gz", &VERSION)
        } else {
            format!("clightning-{}-x86_64-apple-darwin.tar.gz", &VERSION)
        }
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn download_filename() -> String {
        format!("clightning-{}-x86_64-linux-gnu.tar.gz", &VERSION)
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    fn download_filename() -> String {
        format!("clightning-{}-aarch64-linux-gnu.tar.gz", &VERSION)
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    fn download_filename() -> String {
        format!("clightning-{}-win64.zip", &VERSION)
    }
*/
    fn download_filename() -> String {
        let version = ubuntu_version::Version::detect().unwrap();
        format!("clightning-{}-Ubuntu-{}.{:0>2}.tar.xz", &VERSION, version.major, version.minor)
        //format!("clightning-{}-Ubuntu-{}.{:0>2}.tar.xz", &VERSION, "22", "4")
    }
    fn get_expected_sha256(filename: &str) -> anyhow::Result<sha256::Hash> {
        println!("{}",filename);
        let sha256sums_filename = format!("sha256/clightning-{}-SHA256SUMS", &VERSION);
        let sha256sums_filename = format!("{}", sha256sums_filename);
        let file = File::open(&sha256sums_filename)
            .with_context(|| format!("cannot find {:?}", sha256sums_filename))?;
        for line in BufReader::new(file).lines().flatten() {
            let tokens: Vec<_> = line.split("  ").collect();
            if tokens.len() == 2 && filename == tokens[1] {
                return Ok(sha256::Hash::from_str(tokens[0]).unwrap());
            }
        }
        panic!(
            "Couldn't find hash for `{}` in `{}`:\n{}",
            filename,
            sha256sums_filename,
            std::fs::read_to_string(&sha256sums_filename).unwrap()
        );
    }

    pub(crate) fn start() -> anyhow::Result<()> {
        let download_filename = download_filename();
        let expected_hash = get_expected_sha256(&download_filename)?;
        let out_dir = std::env::var_os("OUT_DIR").unwrap();

        let lightning_exe_home = Path::new(&out_dir).join("lightning");
        if !lightning_exe_home.exists() {
            std::fs::create_dir(&lightning_exe_home)
                .with_context(|| format!("cannot create dir {:?}", lightning_exe_home))?;
        }
        let existing_filename = lightning_exe_home
            .join("usr")
            .join("bin")
            .join("lightningd");

        println!(
            "filename:{} version:{} hash:{}",
            download_filename, VERSION, expected_hash
        );

        if !existing_filename.exists() {

            let (file_or_url, tarball_bytes) = match std::env::var("LIGHTNINGD_TARBALL_FILE") {
                Err(_) => {
                    let download_endpoint = std::env::var("LIGHTNINGD_TARBALL_FILE")
                        .unwrap_or("https://github.com/ElementsProject/lightning/releases/download/".to_owned());
                    let url = format!(
                        "{}/{}/{}",
                        download_endpoint, VERSION, download_filename
                    );
                    let resp = minreq::get(&url)
                        .send()
                        .with_context(|| format!("cannot reach url {}", url))?;
                    assert_eq!(resp.status_code, 200, "url {} didn't return 200", url);

                    (url, resp.as_bytes().to_vec())
                }
                Ok(path) => {
                    let f = File::open(&path).with_context(|| {
                        format!(
                            "Cannot find {:?} specified with env var LIGHTNINGD_TARBALL_FILE",
                            &path
                        )
                    })?;
                    let mut reader = BufReader::new(f);
                    let mut buffer = Vec::new();
                    reader.read_to_end(&mut buffer)?;
                    (path, buffer)
                }
            };

            let tarball_hash = sha256::Hash::hash(&tarball_bytes);
            assert_eq!(
                expected_hash, tarball_hash,
                "expected hash of {} is not matching",
                file_or_url
            );

            if download_filename.ends_with(".tar.gz") {
                let d = GzDecoder::new(&tarball_bytes[..]);

                let mut archive = Archive::new(d);
                for mut entry in archive.entries().unwrap().flatten() {
                    if let Ok(file) = entry.path() {
                        if file.ends_with("lightningd") {
                            entry.unpack_in(&lightning_exe_home).unwrap();
                        }
                    }
                }
            } else if download_filename.ends_with(".tar.xz") {
                let d = XzDecoder::new(&tarball_bytes[..]);
                let mut archive = Archive::new(d);
                for mut entry in archive.entries().unwrap().flatten() {
                    if let Ok(file) = entry.path() {
                        if file.ends_with("lightningd") {
                            entry.unpack_in(&lightning_exe_home).unwrap();
                        }
                    }
                }
            }
        }
        Ok(())
    }
}