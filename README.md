[![MIT license](https://img.shields.io/github/license/lvaccaro/lightningd)](https://github.com/lvaccaro/lightningd/blob/master/LICENSE)
[![Crates](https://img.shields.io/crates/v/lightningd.svg)](https://crates.io/crates/lightningd)
[![Docs](https://img.shields.io/badge/docs.rs-lightnignd-green)](https://docs.rs/lightningd)

# Lightningd

Utility to run a regtest Lightningd process, useful in integration testing environment.

When the auto-download feature is selected by activating one of the version feature, such as `v23.05.2`
for lightning core v23.05.2, starting a regtest node is as simple as that:

```rust
// the download feature is enabled whenever a specific version is enabled, for example `v23.05.2`
#[cfg(feature = "download")]
{
  use clightningrpc::LightningRPC;
  let lightningd = lightningd::LightningD::from_downloaded().unwrap();
  assert_eq!(0, lightningd.client.get_info().unwrap().blocks);
}
```

The build script will automatically download the lightning core version v23.05.2 from [lightning core](https://github.com/ElementsProject/lightning),
verify the hashes and place it in the build directory for this crate. If you wish to download from an 
alternate location, for example locally for CI, use the `LIGHTNINGD_DOWNLOAD_ENDPOINT` env var.

When you don't use the auto-download feature you have the following options:

* have `lightning` executable in the `PATH`
* provide the `lightning` executable via the `LIGHTNINGD_EXEC` env var

```rust
use clightningrpc::LightningRPC;
if let Ok(exe_path) = lightningd::exe_path() {
  let lightningd = lightningd::LightningD::new(exe_path).unwrap();
  assert_eq!(0, lightningd.client.get_info().unwrap().blocks);
}
```

Startup options could be configured via the [`Conf`] struct using [`LightningD::with_conf`] or 
[`LightningD::from_downloaded_with_conf`]

## Limitations

Binaries are fetched from [lightning repo](https://github.com/ElementsProject/lightning/).
Support only the following OS/platform:
- Fedora 28 on amd64
- Ubuntu 18.04 on amd64
- Ubuntu 20.04 on amd64
- Ubuntu 22.04 on amd64

If you just have a `lightningd` binary, you could use it by overwriting the path as:
```shell
export LIGHTNINGD_EXE=/usr/bin/lightningd
```
## Thanks

`Clightningd` is inspired by amazing work of [RCasatta]https://github.com/RCasatta) for the [bitcoind](https://github.com/RCasatta/bitcoind) and [elementsd](https://github.com/RCasatta/elementsd) library tools.