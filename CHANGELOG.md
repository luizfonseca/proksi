# Changelog

## [0.1.4](https://github.com/luizfonseca/proksi/compare/v0.1.3...v0.1.4) (2024-05-16)


### Bug Fixes

* **dockerfile:** incorrect image used ([dd44443](https://github.com/luizfonseca/proksi/commit/dd444436a3bcf0dc9815c3cf3e771ef31c3ecded))

## [0.1.3](https://github.com/luizfonseca/proksi/compare/v0.1.2...v0.1.3) (2024-05-16)


### Features

* add non-blocking tracing and static ROUTER with arc_swap ([452f2e7](https://github.com/luizfonseca/proksi/commit/452f2e73e8a462c238404865d51c07f9ff0353cf))
* **config:** add command-line argument support ([2a24aaa](https://github.com/luizfonseca/proksi/commit/2a24aaa3e1c9358e169932e1cc67b73134fbf0d8))
* **config:** add support for providing a config path through the command line first ([63dcd00](https://github.com/luizfonseca/proksi/commit/63dcd00c48cf25d1a15ded8f8a21e204d3fad9f0))
* **config:** enable letsencrypt options via config file ([28aaebc](https://github.com/luizfonseca/proksi/commit/28aaebc23ac5a85d35a3b775304f0b5eb081ce3f))
* **logger:** create lightweight background logging to stdout ([163abb9](https://github.com/luizfonseca/proksi/commit/163abb9e9fc247cfb5b305a829f1c289b0642833))
* **proxy:** enable configuring worker threads through env/command/config ([6af3f5c](https://github.com/luizfonseca/proksi/commit/6af3f5c145d8bbe2eb19a8b699e0d74bfa446a29))
* **proxy:** support reading upstream hosts from configuration ([1885b22](https://github.com/luizfonseca/proksi/commit/1885b22e49f4133825e4749185fbf572a73a1de8))
* use default options for peer requests ([54e2397](https://github.com/luizfonseca/proksi/commit/54e2397efa230869814b4a7cadefa13875250f71))


### Miscellaneous Chores

* release 0.1.3 ([ec9c02d](https://github.com/luizfonseca/proksi/commit/ec9c02d190a1c613509aed0de3eddc20c2c50858))

## [0.1.2](https://github.com/luizfonseca/proksi/compare/v0.1.1...v0.1.2) (2024-05-13)


### Bug Fixes

* **ghactions:** warning on release ([df7e726](https://github.com/luizfonseca/proksi/commit/df7e726be1c7a41e7ed1f0d48ad4f000914b5b32))

## [0.1.1](https://github.com/luizfonseca/proksi/compare/v0.1.0...v0.1.1) (2024-05-13)


### Bug Fixes

* **goreleaser:** attempt on fixing the broken build process ([4e4c989](https://github.com/luizfonseca/proksi/commit/4e4c989f4407f15ec91cd178b8355db29655fa61))

## 0.1.0 (2024-05-13)


### Features

* add configuration based on figments ([48e2981](https://github.com/luizfonseca/proksi/commit/48e2981d3708004272c346299488aabcc13b0ec3))
* add tracing crate & upgrade pingora ([ccf6427](https://github.com/luizfonseca/proksi/commit/ccf64276f27206f6a4bd855b0fd0d28eb07ba457))


### Bug Fixes

* **clippy:** clean clippy warnings ([15afc98](https://github.com/luizfonseca/proksi/commit/15afc989740fdbfe9ea6bb632e438a7ee6dc3d3d))
* readme tasks ([77afc99](https://github.com/luizfonseca/proksi/commit/77afc99370ce6bb8d47b905bd6710f470eb56eaf))
* tests in config ([1a738f6](https://github.com/luizfonseca/proksi/commit/1a738f61e2f73544be36d9b695799bb818ce6b96))
