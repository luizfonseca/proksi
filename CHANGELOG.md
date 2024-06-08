# Changelog

## [0.2.2](https://github.com/luizfonseca/proksi/compare/v0.2.1...v0.2.2) (2024-06-08)


### Bug Fixes

* **docker:** incorrect unwrapping of optional plugin. Resolves [#23](https://github.com/luizfonseca/proksi/issues/23) ([6259725](https://github.com/luizfonseca/proksi/commit/6259725a9307905a40edcafb8d528c257ed49853))

## [0.2.1](https://github.com/luizfonseca/proksi/compare/v0.2.0...v0.2.1) (2024-06-08)


### Features

* add support for plugin configuration ([a572ff8](https://github.com/luizfonseca/proksi/commit/a572ff8114c790f90e412bcdf5807446ef726130))
* **docker:** enable oauth2 plugin docker service labels ([181e69e](https://github.com/luizfonseca/proksi/commit/181e69e06d946162e6ad86e2a3cc54f166104a68))
* **docker:** enable self-signed certs ([6526abb](https://github.com/luizfonseca/proksi/commit/6526abbc6c0e02b2b880d0d219771738c90bc019))
* **oauth2:** add oauth2 validation config support ([a0da2a5](https://github.com/luizfonseca/proksi/commit/a0da2a56334fba376a95b96fba028d36ea965f52))
* **plugins:** add oauth2 plugin - github/workos support ([18ba0ad](https://github.com/luizfonseca/proksi/commit/18ba0ad6301847a6e362aadfae8ad7bd9eb461bc))


### Bug Fixes

* clippy issues ([936abb7](https://github.com/luizfonseca/proksi/commit/936abb7cc7ac41da1bc8fe98ea6099ed9cad3cba))
* clippy issues ([9a9d090](https://github.com/luizfonseca/proksi/commit/9a9d0901b257ea2c16cd3b57333474297f9a087d))
* **docker:** add missing headers remove/add logic to containers ([47c1e4c](https://github.com/luizfonseca/proksi/commit/47c1e4c6e667a6a586420e2b2d50b42233c6aa14))
* **docker:** add missing plugins.oauth2.provider label config ([a7cf5f4](https://github.com/luizfonseca/proksi/commit/a7cf5f4130d557ea32e5388b6c3e0ddc045ecaeb))
* dockerfile and CA certificates ([aeab65e](https://github.com/luizfonseca/proksi/commit/aeab65ef38065c5457f424fe3416444248536601))
* incorrect return on 401 responses ([a2dcaae](https://github.com/luizfonseca/proksi/commit/a2dcaae1bbbcfba2623d9d2b71f5b426f639c1a1))


### Miscellaneous Chores

* release marker for 0.2.1 ([cef795b](https://github.com/luizfonseca/proksi/commit/cef795b10399b18ef810abc35a24e9a3b019cf6f))

## [0.2.0](https://github.com/luizfonseca/proksi/compare/v0.1.11...v0.2.0) (2024-05-31)


### Features

* **config:** add path matcher config for routes ([52e35b9](https://github.com/luizfonseca/proksi/commit/52e35b9f76471f5898134b9b30cc869be6fe3457))
* **config:** enable self_signed option via configuration ([029b74d](https://github.com/luizfonseca/proksi/commit/029b74dfac417fb3c0a4bf3a1be106a5304718fc))
* **docker:** enable header add/remove in docker labels (json format) ([0ef79e1](https://github.com/luizfonseca/proksi/commit/0ef79e1bc334662ed14335414a768445a46b70de))
* **docker:** enable path patterns on docker ([ad87ee3](https://github.com/luizfonseca/proksi/commit/ad87ee3476ee9891ba93c1a571d05aacf32fbf12))
* **lets_encrypt:** generate self-signed certificate for unreachable (local) domains ([37e767a](https://github.com/luizfonseca/proksi/commit/37e767aeea4f4d8a5bcf039c0428c595277ec347))
* **routing:** enable header addition configuration ([547dbaf](https://github.com/luizfonseca/proksi/commit/547dbaf9ce5cc90fce7c0e1a551400ddc83866c0))
* **routing:** enable header removal configuration ([c283cd4](https://github.com/luizfonseca/proksi/commit/c283cd452e716b8dc0291052b8f39d7d4402c549))
* **routing:** enable users to provide PATH patterns against hosts ([e34ec4b](https://github.com/luizfonseca/proksi/commit/e34ec4b8523a7e2208e0d1b74c2a6f33ed9f260f))


### Bug Fixes

* **cert_store:** return SNI error when certificate for domain doesn't exist ([8648cb3](https://github.com/luizfonseca/proksi/commit/8648cb3d07280a9f15563ffa1d5e30507de43b48))
* improve clippy warnings ([e7122de](https://github.com/luizfonseca/proksi/commit/e7122de408c6a0bfd3b83b278a7a932c312ea67d))

## [0.1.11](https://github.com/luizfonseca/proksi/compare/v0.1.10...v0.1.11) (2024-05-27)


### Features

* **docker:** add support for docker swarm service discovery ([185cab3](https://github.com/luizfonseca/proksi/commit/185cab35420bdd8ea91c3ac3725e8badda57b0c9))


### Miscellaneous Chores

* release 0.1.11 ([7f8d3e1](https://github.com/luizfonseca/proksi/commit/7f8d3e16bbab8d045da309ce037d643f0c77a1fc))

## [0.1.10](https://github.com/luizfonseca/proksi/compare/v0.1.9...v0.1.10) (2024-05-27)


### Bug Fixes

* clippy ([b50a511](https://github.com/luizfonseca/proksi/commit/b50a511fc92c141161310589c32ff23ed134ca53))
* small refactorings to services ([606b8dc](https://github.com/luizfonseca/proksi/commit/606b8dcd209122fae20bf8aafbf39fa88a7dbc8f))


### Miscellaneous Chores

* release 0.1.10 ([b852f41](https://github.com/luizfonseca/proksi/commit/b852f41660c56f85abadb5b2debdc1259cd1a586))

## [0.1.9](https://github.com/luizfonseca/proksi/compare/v0.1.8...v0.1.9) (2024-05-27)


### Bug Fixes

* **docker:** incorrect label logic ([63e9dfb](https://github.com/luizfonseca/proksi/commit/63e9dfbb6cf68c468dacb6ae43cf0ca6947d7e7c))


### Miscellaneous Chores

* release 0.1.9 ([185af43](https://github.com/luizfonseca/proksi/commit/185af43de32d180398d1bf4070785d005974ab95))

## [0.1.8](https://github.com/luizfonseca/proksi/compare/v0.1.7...v0.1.8) (2024-05-27)


### Features

* **docker:** add route discovery and container discovery ([61975ec](https://github.com/luizfonseca/proksi/commit/61975ec877a8c2a8a717708542a2c2500dbaf854))
* **docker:** allow proksi.enabled and proksi.enable as possible labels for discovery ([c0384a1](https://github.com/luizfonseca/proksi/commit/c0384a1d2752a99b6c5a0280a4e987b970d0f5c4))
* introduce docker configuration ([780a301](https://github.com/luizfonseca/proksi/commit/780a30177c4edcc119070540a41ea69692c25279))
* **routing:** use tokio broadcast instead of crossbeam ([c3ec7e2](https://github.com/luizfonseca/proksi/commit/c3ec7e2c8e73ec831db5635b53349aa2ff8a00cc))


### Bug Fixes

* **ci:** split build steps ([ef85925](https://github.com/luizfonseca/proksi/commit/ef8592582b87ecfca86ecbc831f82f9b8a33e41a))
* clippy warnings on usize ([1865999](https://github.com/luizfonseca/proksi/commit/1865999671a94903c3f029fec2fca6471b1bca0e))
* correct clippy issues and add makefile ([85ce9aa](https://github.com/luizfonseca/proksi/commit/85ce9aa83e66e3caeda0324678c8fed3be7c1d20))
* high cpu usage on log stream / health checks updating service discovery ([bc80bdf](https://github.com/luizfonseca/proksi/commit/bc80bdf509cd665262d20161321262d32d7b74de))
* incorrect release target on CI ([a4eec6d](https://github.com/luizfonseca/proksi/commit/a4eec6d2065ee299e7269ae82c02b38c0801f396))


### Miscellaneous Chores

* release 0.1.8 ([547e1ed](https://github.com/luizfonseca/proksi/commit/547e1ed7cdea483acd9828bc5290cf1a41eae6b5))

## [0.1.7](https://github.com/luizfonseca/proksi/compare/v0.1.7...v0.1.7) (2024-05-20)


### Bug Fixes

* **ci:** split build steps ([ef85925](https://github.com/luizfonseca/proksi/commit/ef8592582b87ecfca86ecbc831f82f9b8a33e41a))

## [0.1.7](https://github.com/luizfonseca/proksi/compare/v0.1.6...v0.1.7) (2024-05-20)


### Bug Fixes

* **ci:** missing release outputs ([4028460](https://github.com/luizfonseca/proksi/commit/4028460811bffa94a0628a9710105a4d4b2675d2))


### Miscellaneous Chores

* release 0.1.7 ([d867ea4](https://github.com/luizfonseca/proksi/commit/d867ea41bb57a4a105630d05c116577e9bb124f4))

## [0.1.6](https://github.com/luizfonseca/proksi/compare/v0.1.5...v0.1.6) (2024-05-20)


### Features

* **config:** allow users to disable/enable background services ([224c99d](https://github.com/luizfonseca/proksi/commit/224c99d28126ba8cf367d7f0f9ad4c073431798d))
* **config:** validate provided config with sane defaults ([e2727c7](https://github.com/luizfonseca/proksi/commit/e2727c794d755f0d8a76e56053b0049644f48f74))


### Miscellaneous Chores

* marker for release 0.1.6 ([06d57cf](https://github.com/luizfonseca/proksi/commit/06d57cfeb1bdde8ab30732e931943ef10cc204b0))

## [0.1.5](https://github.com/luizfonseca/proksi/compare/v0.1.4...v0.1.5) (2024-05-18)


### Features

* **letsencrypt:** add daily certificate renewal check ([90c14c6](https://github.com/luizfonseca/proksi/commit/90c14c63be0e9595ddca15250e4c20ae3c1a6cec))
* **letsencrypt:** refactor and improve background service logic to handle existing certificates ([27e2564](https://github.com/luizfonseca/proksi/commit/27e2564ff097718324f55bc77e57fd47aa56f404))
* **proxy:** use dashmap for route/cert thread access ([bf55ce8](https://github.com/luizfonseca/proksi/commit/bf55ce8ce44278ba774e0496155d84c9d8d5f05a))


### Bug Fixes

* remove todo from logger flush() ([796b8ef](https://github.com/luizfonseca/proksi/commit/796b8ef6f3493b48543f1298e507424bfd79056f))


### Miscellaneous Chores

* release 0.1.5 ([57cf174](https://github.com/luizfonseca/proksi/commit/57cf174b2d4fa5ae6f043044113975bf7712c4a6))

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
