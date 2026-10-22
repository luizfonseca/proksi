# Changelog

## [0.17.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.16.1...proksi-v0.17.0) (2026-10-22)


### Features

* add `notify` block support for webhook sending based on upstream information ([#562](https://github.com/luizfonseca/proksi/issues/562)) ([98d15a7](https://github.com/luizfonseca/proksi/commit/98d15a76492399b858bf081725dbc08e2b916ba1))
* add body_rewrite to change request bodies as they stream ([#556](https://github.com/luizfonseca/proksi/issues/556)) ([a87001a](https://github.com/luizfonseca/proksi/commit/a87001a03408bc3663b77788e7cfaa08cdd418a9))
* add more information about requests that could not be served by a location ([#558](https://github.com/luizfonseca/proksi/issues/558)) ([8d964d3](https://github.com/luizfonseca/proksi/commit/8d964d35b9530821bd2e932748aa634e0f2f355d))
* count requests no location served by why, instead of all as _unknown ([8d964d3](https://github.com/luizfonseca/proksi/commit/8d964d35b9530821bd2e932748aa634e0f2f355d))
* create bandwidth_limit plugin to help pace responses per client ([#550](https://github.com/luizfonseca/proksi/issues/550)) ([13c63f4](https://github.com/luizfonseca/proksi/commit/13c63f4d924679509a06bfd7abea03c804a272e4))
* create openapi plugin for api validation via specs  ([#557](https://github.com/luizfonseca/proksi/issues/557)) ([1cb1a0f](https://github.com/luizfonseca/proksi/commit/1cb1a0f947392074e2b40f17a73fb2720e6f2463))
* create waiting_room plugin for visitor-type-based locations ([#552](https://github.com/luizfonseca/proksi/issues/552)) ([7691625](https://github.com/luizfonseca/proksi/commit/76916251d9afd4b80cc13efacb261eade67e4812))
* sign in to the admin UI with a token's name and secret ([#560](https://github.com/luizfonseca/proksi/issues/560)) ([a8c6027](https://github.com/luizfonseca/proksi/commit/a8c602795b6270c8695e3ab45eda01800424ad93))


### Performance Improvements

* find the client once per request for rules, and read headers by prebuilt names ([#555](https://github.com/luizfonseca/proksi/issues/555)) ([650bf0c](https://github.com/luizfonseca/proksi/commit/650bf0cfece90ab348236a375f14888ff2487667))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.16.1 to 0.17.0
    * proksi_plugins bumped from 0.16.1 to 0.17.0
    * proksi_store bumped from 0.16.1 to 0.17.0
  * dev-dependencies
    * proksi_plugins bumped from 0.16.1 to 0.17.0
    * proksi_store bumped from 0.16.1 to 0.17.0

## [0.16.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.16.0...proksi-v0.16.1) (2026-10-21)


### Bug Fixes

* decode escaped letters and dots before matching paths ([#541](https://github.com/luizfonseca/proksi/issues/541)) ([262e50e](https://github.com/luizfonseca/proksi/commit/262e50e96ae58f1bee766d65aa79654af3a20795))
* spell out the paths blocked in the URL scanners recipe ([#542](https://github.com/luizfonseca/proksi/issues/542)) ([52aab11](https://github.com/luizfonseca/proksi/commit/52aab11ef32ccf94d9ea907158833bf73224e717))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.16.0 to 0.16.1
    * proksi_store bumped from 0.16.0 to 0.16.1
  * dev-dependencies
    * proksi_store bumped from 0.16.0 to 0.16.1

## [0.16.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.15.1...proksi-v0.16.0) (2026-10-21)


### Features

* add exact paths and regex locations ([#529](https://github.com/luizfonseca/proksi/issues/529)) ([a660954](https://github.com/luizfonseca/proksi/commit/a660954603daca477413f319da227f23d099c926))
* check IP and text lists at any size using Aho Corasick ([#525](https://github.com/luizfonseca/proksi/issues/525)) ([c5f2f7a](https://github.com/luizfonseca/proksi/commit/c5f2f7a97a21f867daea1d845a07cee23f681bce))
* history and rollback in the admin API ([#535](https://github.com/luizfonseca/proksi/issues/535)) ([f9da854](https://github.com/luizfonseca/proksi/commit/f9da854c75408b66841f829b47e26286f8877870))
* keep trusted request IDs and use UUID v7 ([#528](https://github.com/luizfonseca/proksi/issues/528)) ([534f9b6](https://github.com/luizfonseca/proksi/commit/534f9b6999e89d518403963e1f2f21e6dcab98cc))
* keep versions of stored hosts and roll them back ([#534](https://github.com/luizfonseca/proksi/issues/534)) ([c57942e](https://github.com/luizfonseca/proksi/commit/c57942e522bf0275d4adc58f6bca38d46ce5fe47))
* match lists of values in rules at any size ([#523](https://github.com/luizfonseca/proksi/issues/523)) ([0f7606e](https://github.com/luizfonseca/proksi/commit/0f7606eecc8fc54f027f6c88f363e446a8795603))


### Bug Fixes

* build the dashmap_arc bench ([#531](https://github.com/luizfonseca/proksi/issues/531)) ([38dcd53](https://github.com/luizfonseca/proksi/commit/38dcd53d1a76c5003492eea40acc5e24e1419baa))
* set config headers without copying their names on each request ([#526](https://github.com/luizfonseca/proksi/issues/526)) ([d1afe3f](https://github.com/luizfonseca/proksi/commit/d1afe3f8bef4381f513f604653ac8aab0efe38f4))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.15.1 to 0.16.0
    * proksi_store bumped from 0.15.1 to 0.16.0
  * dev-dependencies
    * proksi_store bumped from 0.15.1 to 0.16.0

## [0.15.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.15.0...proksi-v0.15.1) (2026-10-20)


### Bug Fixes

* pick upstreams and run rules without allocating ([#519](https://github.com/luizfonseca/proksi/issues/519)) ([5c075d4](https://github.com/luizfonseca/proksi/commit/5c075d441d16470628625d3e48892f22754b5b19))
* refuse TLS handshakes for unknown names ([#518](https://github.com/luizfonseca/proksi/issues/518)) ([defd7f1](https://github.com/luizfonseca/proksi/commit/defd7f18c2b4ead6eb914531ff8f0ae5794934ff))
* TLS handshakes for unknown names refused before looking for a certificate ([defd7f1](https://github.com/luizfonseca/proksi/commit/defd7f18c2b4ead6eb914531ff8f0ae5794934ff))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.15.0 to 0.15.1

## [0.15.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.14.2...proksi-v0.15.0) (2026-10-20)


### Features

* request mirroring ([#508](https://github.com/luizfonseca/proksi/issues/508)) ([3d09f3e](https://github.com/luizfonseca/proksi/commit/3d09f3e189b30ec4f1d322d90a1e68adaad1632d))
* retries and least connections ([#509](https://github.com/luizfonseca/proksi/issues/509)) ([8c3237e](https://github.com/luizfonseca/proksi/commit/8c3237e4db8418bc228d40a6dde1a12e615e1d81))
* traffic splits ([#504](https://github.com/luizfonseca/proksi/issues/504)) ([e3e0255](https://github.com/luizfonseca/proksi/commit/e3e02557ddc142ae56ade6b104f774d26debb52c))


### Bug Fixes

* conditionals and loops nested twice at most ([#511](https://github.com/luizfonseca/proksi/issues/511)) ([377d9e5](https://github.com/luizfonseca/proksi/commit/377d9e5a94d185be59e4b18bf0bd975a8be5a35f))
* deny unwrap, expect, panic, todo and unimplemented ([#512](https://github.com/luizfonseca/proksi/issues/512)) ([7bb8066](https://github.com/luizfonseca/proksi/commit/7bb8066e4bfb95a64c3fa71b8097cb6ef800d346))
* jwt checks signatures with ring ([#506](https://github.com/luizfonseca/proksi/issues/506)) ([c68f306](https://github.com/luizfonseca/proksi/commit/c68f3068c1058cdcffbdd5b4c33d358c1b2e20c7))
* no extra work for splits' returning clients and hosts without wildcards ([#507](https://github.com/luizfonseca/proksi/issues/507)) ([61d7df6](https://github.com/luizfonseca/proksi/commit/61d7df646d197cfed7667eae710a5aec5bb98cfe))
* static_files reads files on the request's thread ([#505](https://github.com/luizfonseca/proksi/issues/505)) ([7071843](https://github.com/luizfonseca/proksi/commit/7071843f2b6e2030e3c03ac8a9ba6c64bd5b4689))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.14.2 to 0.15.0

## [0.1.2](https://github.com/luizfonseca/proksi/compare/plugins_api-v0.1.1...plugins_api-v0.1.2) (2026-10-20)


### Bug Fixes

* deny unwrap, expect, panic, todo and unimplemented ([#512](https://github.com/luizfonseca/proksi/issues/512)) ([7bb8066](https://github.com/luizfonseca/proksi/commit/7bb8066e4bfb95a64c3fa71b8097cb6ef800d346))

## [0.14.2](https://github.com/luizfonseca/proksi/compare/proksi-v0.14.1...proksi-v0.14.2) (2026-10-19)


### Miscellaneous Chores

* **proksi:** Synchronize proksi versions


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.14.1 to 0.14.2

## [0.14.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.14.0...proksi-v0.14.1) (2026-10-19)


### Bug Fixes

* tcp blocks in the admin UI ([#492](https://github.com/luizfonseca/proksi/issues/492)) ([e608648](https://github.com/luizfonseca/proksi/commit/e6086488e0797e72064ea0bbf3cc89a3088aeae7))
* tcp connections closed before any bytes aren't errors ([#494](https://github.com/luizfonseca/proksi/issues/494)) ([8400796](https://github.com/luizfonseca/proksi/commit/8400796dd1179bfba9b6d422bd7ce12ae17efc3d))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.14.0 to 0.14.1

## [0.14.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.13.1...proksi-v0.14.0) (2026-10-19)


### Features

* trusted origins for the admin UI ([#488](https://github.com/luizfonseca/proksi/issues/488)) ([f83ccf7](https://github.com/luizfonseca/proksi/commit/f83ccf7d02cbf8f4783705d6ab03bc2fa9af9304))


### Bug Fixes

* release the admin UI crate with proksi ([#491](https://github.com/luizfonseca/proksi/issues/491)) ([90834fd](https://github.com/luizfonseca/proksi/commit/90834fdb4b4862c8d5cc387ca32d6da70cc0ffe0))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi-admin-ui bumped from 0.13.1 to 0.14.0

## [0.13.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.13.0...proksi-v0.13.1) (2026-10-19)


### Bug Fixes

* admin UI version in releases ([#486](https://github.com/luizfonseca/proksi/issues/486)) ([c1c1f80](https://github.com/luizfonseca/proksi/commit/c1c1f8009fef48b4918fec8fa61bec3a62df3000))

## [0.13.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.12.0...proksi-v0.13.0) (2026-10-19)


### Features

* rule builder ([#480](https://github.com/luizfonseca/proksi/issues/480)) ([104e47a](https://github.com/luizfonseca/proksi/commit/104e47ab0e02ecc184a42c4886f53acf36660aa3))


### Bug Fixes

* error responses over HTTP/2 ([#484](https://github.com/luizfonseca/proksi/issues/484)) ([87bf40a](https://github.com/luizfonseca/proksi/commit/87bf40aeb1e423e8c44c74222afe687842755aab))
* latest dependencies ([#481](https://github.com/luizfonseca/proksi/issues/481)) ([9696f89](https://github.com/luizfonseca/proksi/commit/9696f890dc1899567363cc0eb3fad9a57dc017b8))

## [0.1.1](https://github.com/luizfonseca/proksi/compare/plugins_api-v0.1.0...plugins_api-v0.1.1) (2026-10-19)


### Bug Fixes

* latest dependencies ([#481](https://github.com/luizfonseca/proksi/issues/481)) ([9696f89](https://github.com/luizfonseca/proksi/commit/9696f890dc1899567363cc0eb3fad9a57dc017b8))

## [0.12.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.11.1...proksi-v0.12.0) (2026-10-18)


### Features

* cors, to let pages of other origins call a location ([#461](https://github.com/luizfonseca/proksi/issues/461)) ([718ac2a](https://github.com/luizfonseca/proksi/commit/718ac2a129c24d80fb2aa4cd6f7f072f1598447e))
* forward_auth, to let an auth service decide on each request ([#464](https://github.com/luizfonseca/proksi/issues/464)) ([6800e70](https://github.com/luizfonseca/proksi/commit/6800e70de57133f2ce8d4d25018484f41a2c73db))
* ip_filter, to accept or refuse clients by address ([#460](https://github.com/luizfonseca/proksi/issues/460)) ([6b78070](https://github.com/luizfonseca/proksi/commit/6b780700b2072121b2adcf91476cafff79d83614))
* rules, conditions on requests and what to do when they hold ([#466](https://github.com/luizfonseca/proksi/issues/466)) ([9cd815f](https://github.com/luizfonseca/proksi/commit/9cd815f6dae4d0de582b5739e6caf146f60b56b8))


### Bug Fixes

* a rule's condition is always an and, or or not block ([#467](https://github.com/luizfonseca/proksi/issues/467)) ([78b4682](https://github.com/luizfonseca/proksi/commit/78b4682d551eb12f479efb9f0a96d8ad48734525))
* client addresses in kubernetes ([#470](https://github.com/luizfonseca/proksi/issues/470)) ([a7d4ad6](https://github.com/luizfonseca/proksi/commit/a7d4ad68d182743381a08287dff7629cee10e989))
* give each test proksi different ports ([#463](https://github.com/luizfonseca/proksi/issues/463)) ([ace4f1e](https://github.com/luizfonseca/proksi/commit/ace4f1e87ed4d30d32fcfd6d5ff59d83cc29eb21))

## [0.11.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.11.0...proksi-v0.11.1) (2026-10-18)


### Bug Fixes

* keep mimalloc from reserving an arena in the supervisor ([#452](https://github.com/luizfonseca/proksi/issues/452)) ([e34edc1](https://github.com/luizfonseca/proksi/commit/e34edc1fe2940ac3bbca8863b882316b01aa5270))


### Performance Improvements

* fewer allocations per request ([#453](https://github.com/luizfonseca/proksi/issues/453)) ([db23cbf](https://github.com/luizfonseca/proksi/commit/db23cbf3de52b337c5a1b442da0fb73882c0174b))

## [0.11.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.10.0...proksi-v0.11.0) (2026-10-17)


### Features

* admin.metrics_listen, a listener serving only /metrics ([#445](https://github.com/luizfonseca/proksi/issues/445)) ([4d3355e](https://github.com/luizfonseca/proksi/commit/4d3355e1f4eff82ca6455c3ab6b9ea2802125708))
* body_secs, a limit on how long a request body can take to arrive ([#443](https://github.com/luizfonseca/proksi/issues/443)) ([e8a42f1](https://github.com/luizfonseca/proksi/commit/e8a42f138308e254dcb7ffe6bda6f2a9eeac161a))
* count tcp upstream errors and failed TLS handshakes ([#447](https://github.com/luizfonseca/proksi/issues/447)) ([aab93ec](https://github.com/luizfonseca/proksi/commit/aab93ece484a4463939f822481feffc1cc72f638))
* proksi_responses_total, responses by status code ([#446](https://github.com/luizfonseca/proksi/issues/446)) ([d9db3a0](https://github.com/luizfonseca/proksi/commit/d9db3a02d6249b9fb6ee42317520687f6008f984))
* server.tls sets the TLS versions and ciphers accepted, without DHE by default ([#444](https://github.com/luizfonseca/proksi/issues/444)) ([ab0c38d](https://github.com/luizfonseca/proksi/commit/ab0c38d7291e6460574966df80c66529e0bf59a2))


### Bug Fixes

* create the in-memory cache when it's first used ([#451](https://github.com/luizfonseca/proksi/issues/451)) ([263a27f](https://github.com/luizfonseca/proksi/commit/263a27f7ed3a013b2aea9b266d2c5b1c32b63978))
* don't log clients giving up on TLS handshakes as errors ([#448](https://github.com/luizfonseca/proksi/issues/448)) ([d196958](https://github.com/luizfonseca/proksi/commit/d196958db04cbc3b282390d46690b1b8720201d3))
* proksi's own responses get the configured headers, and no Server header ([#441](https://github.com/luizfonseca/proksi/issues/441)) ([20e3c3d](https://github.com/luizfonseca/proksi/commit/20e3c3d4a9b88ab7f8eaaa4164ccd11d006f286a))
* start the supervisor before loading the configuration ([#450](https://github.com/luizfonseca/proksi/issues/450)) ([632ea55](https://github.com/luizfonseca/proksi/commit/632ea55bbeb03ada9f93a73d700a03d4871a7b94))
* turn off transparent huge pages, which more than doubled memory ([#449](https://github.com/luizfonseca/proksi/issues/449)) ([058ab2f](https://github.com/luizfonseca/proksi/commit/058ab2fb3455d55074bc2ed210de42b4f80fc2f0))

## [0.10.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.9.0...proksi-v0.10.0) (2026-10-17)


### Features

* timeouts toward upstreams, and for idle tcp connections ([#438](https://github.com/luizfonseca/proksi/issues/438)) ([67d39e4](https://github.com/luizfonseca/proksi/commit/67d39e45ced1cde1a1d9fdda4a74259d41c6c6d3))


### Bug Fixes

* send requests to a replaced upstream as soon as it's up ([#439](https://github.com/luizfonseca/proksi/issues/439)) ([2b4d36b](https://github.com/luizfonseca/proksi/commit/2b4d36bbbca9bd08de120f507e26989719e2561f))
* wait for the rate limit metrics in test_admin_metrics ([#436](https://github.com/luizfonseca/proksi/issues/436)) ([048c26a](https://github.com/luizfonseca/proksi/commit/048c26a69574282097c22a85dde7974dc6f5dfc4))

## [0.9.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.8.0...proksi-v0.9.0) (2026-10-17)


### Features

* rewrite paths with regular expressions ([#435](https://github.com/luizfonseca/proksi/issues/435)) ([976c13e](https://github.com/luizfonseca/proksi/commit/976c13e7a719e3ddf82126f363e89bfa7f21b2b7))
* rewrite the path sent to upstreams, per location ([#433](https://github.com/luizfonseca/proksi/issues/433)) ([4e6ed48](https://github.com/luizfonseca/proksi/commit/4e6ed4807efc5582ead574f7cf5545800e8e402d))
* upgrade to a new worker without dropping connections (Linux) ([#427](https://github.com/luizfonseca/proksi/issues/427)) ([d57e88b](https://github.com/luizfonseca/proksi/commit/d57e88b7acc1515bb19470ed35dd5bea77ddce63))


### Bug Fixes

* tcp connect timeout, and follow upstream containers that are removed ([#430](https://github.com/luizfonseca/proksi/issues/430)) ([3b96ab7](https://github.com/luizfonseca/proksi/commit/3b96ab7e8a28501c2bd0ddeeb76195e028655e6a))
* work with cert-manager's HTTP-01 solver, and the ingress class annotation ([#428](https://github.com/luizfonseca/proksi/issues/428)) ([06ad194](https://github.com/luizfonseca/proksi/commit/06ad1944cfcaa0027d76b2df3a3379d11c9ce29e))

## [0.8.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.7.0...proksi-v0.8.0) (2026-10-17)


### Features

* certificates of kubernetes ingresses from their tls secrets, and upstreams through services ([#416](https://github.com/luizfonseca/proksi/issues/416)) ([93832db](https://github.com/luizfonseca/proksi/commit/93832dbeb18338980dba068efcd67be26ae4f30c))
* kubernetes plugins through annotations, ingress status, events and a helm chart ([#417](https://github.com/luizfonseca/proksi/issues/417)) ([b32db89](https://github.com/luizfonseca/proksi/commit/b32db89f4fff2d317ab7879da7cb8baf29f8d59a))
* record metrics only with the admin api, and allow turning them off ([#419](https://github.com/luizfonseca/proksi/issues/419)) ([acfec67](https://github.com/luizfonseca/proksi/commit/acfec67697d5e0104bd2af15272c83b16ba999e2))
* reload certificate files when they change ([#423](https://github.com/luizfonseca/proksi/issues/423)) ([a256748](https://github.com/luizfonseca/proksi/commit/a2567489de4d630c218dea34f370a297002674ed)), closes [#278](https://github.com/luizfonseca/proksi/issues/278)
* resolve upstream names again, following their addresses ([#424](https://github.com/luizfonseca/proksi/issues/424)) ([ddf7bf2](https://github.com/luizfonseca/proksi/commit/ddf7bf2c2a5d82f69c0454ac55d9c4ce64357370))
* send x-forwarded-for, -proto, -host and x-real-ip to upstreams ([#418](https://github.com/luizfonseca/proksi/issues/418)) ([5796f58](https://github.com/luizfonseca/proksi/commit/5796f589dff3075df02646e39dc42ee63a250d9b))
* stop once connections are closed, waiting up to server.shutdown_timeout_secs ([#425](https://github.com/luizfonseca/proksi/issues/425)) ([1f05596](https://github.com/luizfonseca/proksi/commit/1f0559687c617fa6730d1278b58e52845fdb55f8))
* timeouts toward clients, in server.timeouts ([#426](https://github.com/luizfonseca/proksi/issues/426)) ([6d699a3](https://github.com/luizfonseca/proksi/commit/6d699a31fce611b76c98a1d43802d08439e9c4ff))


### Bug Fixes

* serve certificates with every intermediate of their chain ([#421](https://github.com/luizfonseca/proksi/issues/421)) ([61ee6f7](https://github.com/luizfonseca/proksi/commit/61ee6f7198b47481a3798066f4c6ff1312385928)), closes [#420](https://github.com/luizfonseca/proksi/issues/420)
* use the proksi.ingress prefix for kubernetes annotations and labels ([#422](https://github.com/luizfonseca/proksi/issues/422)) ([43c96ea](https://github.com/luizfonseca/proksi/commit/43c96ea247d448441a15e4a2d97635646e6cffd0))

## [0.7.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.6.2...proksi-v0.7.0) (2026-10-16)


### Features

* limit request body sizes per location ([#401](https://github.com/luizfonseca/proksi/issues/401)) ([cb4ca96](https://github.com/luizfonseca/proksi/commit/cb4ca965c842b5c47013fde944ea8b5f4a5b0782))
* load balance upstreams by their weight ([#398](https://github.com/luizfonseca/proksi/issues/398)) ([2c167b1](https://github.com/luizfonseca/proksi/commit/2c167b146474febcbd403e7d32ae93340daf57dd))
* proxy tcp connections with tcp blocks ([#402](https://github.com/luizfonseca/proksi/issues/402)) ([e1ef474](https://github.com/luizfonseca/proksi/commit/e1ef4741e91f1fa83c2a9ba8510f432f91768b88))
* rate limit requests per location and client ([#400](https://github.com/luizfonseca/proksi/issues/400)) ([fdc1f1e](https://github.com/luizfonseca/proksi/commit/fdc1f1e22fa7f1c99d8fd25865d822a9c03ec2ac))
* run plugins, headers and cache as building blocks with a priority ([#399](https://github.com/luizfonseca/proksi/issues/399)) ([34b50ce](https://github.com/luizfonseca/proksi/commit/34b50ce0520dd06bbd31fe7c52b8bed2af610398))
* share the https port with tcp blocks, chosen by sni and alpn ([#403](https://github.com/luizfonseca/proksi/issues/403)) ([75c5e80](https://github.com/luizfonseca/proksi/commit/75c5e80d4d112f3cbf0f230546fadac7b31ee1b1))


### Performance Improvements

* share locations with requests and use mimalloc ([#395](https://github.com/luizfonseca/proksi/issues/395)) ([a305149](https://github.com/luizfonseca/proksi/commit/a3051491338e1be079063d88775107a4e0d15ec2))

## [0.6.2](https://github.com/luizfonseca/proksi/compare/proksi-v0.6.1...proksi-v0.6.2) (2026-10-14)


### Bug Fixes

* route discovery stopping and upstream lookup panic under load ([#371](https://github.com/luizfonseca/proksi/issues/371)) ([ce49a3f](https://github.com/luizfonseca/proksi/commit/ce49a3f1b942f28abce929cfe8ebf944cf5189a6))

## [0.6.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.6.0...proksi-v0.6.1) (2025-07-19)


### Bug Fixes

* configuration file loading to support direct file paths and improve Let's Encrypt validation ([#305](https://github.com/luizfonseca/proksi/issues/305)) ([2a438e0](https://github.com/luizfonseca/proksi/commit/2a438e0eac91824785970cba929fb7a0a3d991db))

## [0.6.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.5.3...proksi-v0.6.0) (2025-05-12)


### Features

* add store adapter trait to store certificates using Redis or In-Memory storage  ([#258](https://github.com/luizfonseca/proksi/issues/258)) ([2503c5d](https://github.com/luizfonseca/proksi/commit/2503c5d6d09a526c7a4cc04f8584e450eeec3804))

## [0.5.3](https://github.com/luizfonseca/proksi/compare/proksi-v0.5.3...proksi-v0.5.3) (2025-01-04)


### Bug Fixes

* allow https and http addresses to be provided via configuration ([bb52d17](https://github.com/luizfonseca/proksi/commit/bb52d173d11c4506e5bcae5c0a38c2bb7c56aba5))
* avoid renewal e-mails from Let's Encrypt ([#183](https://github.com/luizfonseca/proksi/issues/183)) ([0ab5c12](https://github.com/luizfonseca/proksi/commit/0ab5c121504638c259a2501d21328f1f384e1282))
* failing certificate renewal causing a panic due to .unwrap() call ([6e11bc9](https://github.com/luizfonseca/proksi/commit/6e11bc938f2feb72de898530f774df8f56421d5e))
* linting ([5de49ba](https://github.com/luizfonseca/proksi/commit/5de49ba28975da8b89b648e492653498539ef06a))

## [0.5.2](https://github.com/luizfonseca/proksi/compare/proksi-v0.5.2...proksi-v0.5.2) (2025-01-04)


### Bug Fixes

* allow https and http addresses to be provided via configuration ([bb52d17](https://github.com/luizfonseca/proksi/commit/bb52d173d11c4506e5bcae5c0a38c2bb7c56aba5))
* avoid renewal e-mails from Let's Encrypt ([#183](https://github.com/luizfonseca/proksi/issues/183)) ([0ab5c12](https://github.com/luizfonseca/proksi/commit/0ab5c121504638c259a2501d21328f1f384e1282))
* failing certificate renewal causing a panic due to .unwrap() call ([6e11bc9](https://github.com/luizfonseca/proksi/commit/6e11bc938f2feb72de898530f774df8f56421d5e))
* linting ([5de49ba](https://github.com/luizfonseca/proksi/commit/5de49ba28975da8b89b648e492653498539ef06a))

## [0.5.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.5.1...proksi-v0.5.1) (2025-01-04)


### Bug Fixes

* allow https and http addresses to be provided via configuration ([bb52d17](https://github.com/luizfonseca/proksi/commit/bb52d173d11c4506e5bcae5c0a38c2bb7c56aba5))
* failing certificate renewal causing a panic due to .unwrap() call ([6e11bc9](https://github.com/luizfonseca/proksi/commit/6e11bc938f2feb72de898530f774df8f56421d5e))
* linting ([5de49ba](https://github.com/luizfonseca/proksi/commit/5de49ba28975da8b89b648e492653498539ef06a))

## [0.5.1](https://github.com/luizfonseca/proksi/compare/proksi-v0.5.0...proksi-v0.5.1) (2025-01-04)


### Bug Fixes

* avoid renewal e-mails from Let's Encrypt ([#183](https://github.com/luizfonseca/proksi/issues/183)) ([0ab5c12](https://github.com/luizfonseca/proksi/commit/0ab5c121504638c259a2501d21328f1f384e1282))

## [0.5.0](https://github.com/luizfonseca/proksi/compare/proksi-v0.4.8...proksi-v0.5.0) (2024-11-03)


### Features

* upgrade pingora to 0.4.0 ([4dd5ba5](https://github.com/luizfonseca/proksi/commit/4dd5ba562b818138f9f4267f18de30ccdffad473))


### Bug Fixes

* pingora version bump and new struct props ([1304192](https://github.com/luizfonseca/proksi/commit/1304192e04b2dc69a6b8c001d996b3249f70cc01))

## [0.4.8](https://github.com/luizfonseca/proksi/compare/v0.4.7...v0.4.8) (2024-10-19)


### Features

* enable lets encrypt renewal check interval to be configured ([c26dd34](https://github.com/luizfonseca/proksi/commit/c26dd34b05f50023cd40e30723e08fe97468ca00))


### Miscellaneous Chores

* release marker for 0.4.8 ([a0005e0](https://github.com/luizfonseca/proksi/commit/a0005e041be76f9e38bae316097680559730c53a))

## [0.4.7](https://github.com/luizfonseca/proksi/compare/v0.4.6...v0.4.7) (2024-10-19)


### Bug Fixes

* check to renew certificates on server start/restart ([f83bc15](https://github.com/luizfonseca/proksi/commit/f83bc153dbfae0087a03c6278a327444e38366b6))

## [0.4.6](https://github.com/luizfonseca/proksi/compare/v0.4.5...v0.4.6) (2024-10-19)


### Bug Fixes

* panic when handling certificate dates that contain space-padded days ([2a0f447](https://github.com/luizfonseca/proksi/commit/2a0f447bf0635761bb701db7cd820fa068c68cde))

## [0.4.5](https://github.com/luizfonseca/proksi/compare/v0.4.4...v0.4.5) (2024-08-14)


### Bug Fixes

* remove dashmap usage from cache storage ([d721029](https://github.com/luizfonseca/proksi/commit/d7210296e413ba3bb6eecdb72254c4dfae099c68))
* replace dashmap with a lockfree alternative for routing ([4013f8e](https://github.com/luizfonseca/proksi/commit/4013f8e79ffefc34e9a5e4706a3ae12cf99322ea))

## [0.4.4](https://github.com/luizfonseca/proksi/compare/v0.4.3...v0.4.4) (2024-07-13)


### Bug Fixes

* use correct debian docker image (with glibc) ([7729e62](https://github.com/luizfonseca/proksi/commit/7729e62b1dff9bcc061f4d0182593fb1e8e4362e))

## [0.4.3](https://github.com/luizfonseca/proksi/compare/v0.4.2...v0.4.3) (2024-07-13)


### Features

* upgrade to pingora 0.3.0 ([bd5f98f](https://github.com/luizfonseca/proksi/commit/bd5f98fc9c7e3e9d0ab2a25c37830b7eb85033eb))


### Miscellaneous Chores

* release marker for 0.4.3 ([9b7a72c](https://github.com/luizfonseca/proksi/commit/9b7a72cc8ae55555e398899e84bcb2c817c09b3a))

## [0.4.2](https://github.com/luizfonseca/proksi/compare/v0.4.1...v0.4.2) (2024-06-28)


### Bug Fixes

* use absolute paths where possible ([0aab38e](https://github.com/luizfonseca/proksi/commit/0aab38e20615b007a58b1701225b812a5cea2d50))

## [0.4.1](https://github.com/luizfonseca/proksi/compare/v0.4.0...v0.4.1) (2024-06-28)


### Bug Fixes

* auto_reload spawns a service before closing current ([ed89d9b](https://github.com/luizfonseca/proksi/commit/ed89d9b429d035bdc5751cbf0187ca1567f3334b))
* check if saved disk matches memory ([4a3f33e](https://github.com/luizfonseca/proksi/commit/4a3f33ee8c90dcdc1709b2cc093a215ae6b01c56))
* disable logging skips all logging ([e107bb1](https://github.com/luizfonseca/proksi/commit/e107bb187e506e27b1b9da950ca9cb4212dee171))
* use a pingora-compatible rolling logger service ([e4ca77c](https://github.com/luizfonseca/proksi/commit/e4ca77c6bd0f0073f3eee8c84e3fd185b40cff8e))

## [0.4.0](https://github.com/luizfonseca/proksi/compare/v0.3.31...v0.4.0) (2024-06-27)


### Features

* add auto_reload configuration ([2401c32](https://github.com/luizfonseca/proksi/commit/2401c32fbaf0ccfdea658a7ddc316782fdeabbbb))
* **config:** allow auto_reload for specific paths besides the config file ([7e2a7ac](https://github.com/luizfonseca/proksi/commit/7e2a7ac1d0159ac1567c130a5579a86c59587d73))
* **hcl:** add num_cpus() function ([1e7dd91](https://github.com/luizfonseca/proksi/commit/1e7dd914a823fb7d1e249bb55f0e2902613ad7e8))
* set worker_threads to the default number of logical cpus [#69](https://github.com/luizfonseca/proksi/issues/69) ([dfa23e7](https://github.com/luizfonseca/proksi/commit/dfa23e7150f691dd8d3dcd1cd76f8fce0edee6e1))
* watch for changes in the proksi.hcl file ([a5b4b0c](https://github.com/luizfonseca/proksi/commit/a5b4b0c87fb56e52d0a80be270e021921a1de702))


### Bug Fixes

* access_logs filtering and function update ([40df3da](https://github.com/luizfonseca/proksi/commit/40df3dae9563100b42d3eab29c00924653add14e))
* append .log to log files ([49ac548](https://github.com/luizfonseca/proksi/commit/49ac548de63af6d28d368d7ca81f388795d639f6))
* **cache:** add cache-duration header ([a319c06](https://github.com/luizfonseca/proksi/commit/a319c06ab8b6329a8a5440c4484de4c02e73e34d))
* **cache:** use primary_bin hex as cache keys ([c95a9da](https://github.com/luizfonseca/proksi/commit/c95a9dac2542c3ceca2f7694598e7395569b343e))
* **config:** watch correct hcl file extensions ([133486a](https://github.com/luizfonseca/proksi/commit/133486ad226cda84a22d352333f59549b8d44e0b))
* get real number of logical cores ([31c1174](https://github.com/luizfonseca/proksi/commit/31c117495b85e71bc0628f079a86f053e42ed96f))
* remove log color when logging path is set ([0c260e9](https://github.com/luizfonseca/proksi/commit/0c260e959715021190cc1b3e619e05961804e751))

## [0.3.31](https://github.com/luizfonseca/proksi/compare/v0.3.30...v0.3.31) (2024-06-24)


### Bug Fixes

* remove ansi colors for logging to file ([9b1b91d](https://github.com/luizfonseca/proksi/commit/9b1b91ddb90986ab7502786fedceabeaaff6f0b1))

## [0.3.30](https://github.com/luizfonseca/proksi/compare/v0.3.23...v0.3.30) (2024-06-24)


### Features

* **logging:** enable file/stdout log rotation via config file/cli ([6bd378f](https://github.com/luizfonseca/proksi/commit/6bd378fcf6a79578c34d07477d8d6487f77f4003))


### Bug Fixes

* remove default path from logging ([79e144d](https://github.com/luizfonseca/proksi/commit/79e144df51af01b73f0739b26bf70333ad785b68))
* remove fetch of e5 certificate ([eabe6d5](https://github.com/luizfonseca/proksi/commit/eabe6d5a4f013c35925ae1a7e9af70ae41843da8))
* split lets_encrypt certs into leaf/intermediate ([f249a8c](https://github.com/luizfonseca/proksi/commit/f249a8c9aa7fa7054e4e2235854ad1bd7122fcbf))
* use chain certs ([b1d2dcb](https://github.com/luizfonseca/proksi/commit/b1d2dcbc8c1c3025161e13edaa8cfc1ec8692787))


### Miscellaneous Chores

* release marker for 0.3.30 ([a7ac130](https://github.com/luizfonseca/proksi/commit/a7ac130d7a72e58e00f4d826f5a6359a798664cb))

## [0.3.23](https://github.com/luizfonseca/proksi/compare/v0.3.22...v0.3.23) (2024-06-23)


### Bug Fixes

* ci ([82e234c](https://github.com/luizfonseca/proksi/commit/82e234cf0a57106deb405c955900f8626b8ade5d))

## [0.3.22](https://github.com/luizfonseca/proksi/compare/v0.3.21...v0.3.22) (2024-06-23)


### Bug Fixes

* **cert:** add certs to chain. Fixes [#47](https://github.com/luizfonseca/proksi/issues/47) ([05f366c](https://github.com/luizfonseca/proksi/commit/05f366cbaca24dac388fb14907571fd6ad1bf99b))
* change acme-lib to acme-v2 due to security bug ([201fbb6](https://github.com/luizfonseca/proksi/commit/201fbb62c19b3f110a96ef911813947de76c4dcb))
* **http01:** fetch lets_encrypt intermediate cert ([f2c748e](https://github.com/luizfonseca/proksi/commit/f2c748eed5bc0f876e6589f63e804377653f1aab))

## [0.3.21](https://github.com/luizfonseca/proksi/compare/v0.3.20...v0.3.21) (2024-06-22)


### Bug Fixes

* hcl imports work on nested elements ([bf719b6](https://github.com/luizfonseca/proksi/commit/bf719b6b3b86ce0babcf3f9537c31a0531f72a69))

## [0.3.20](https://github.com/luizfonseca/proksi/compare/v0.3.15...v0.3.20) (2024-06-22)


### Features

* **cache:** add in-memory cache for results from disk ([b35f04c](https://github.com/luizfonseca/proksi/commit/b35f04cdeef455d8e5a7cfe0068bb1c90e4c71b8))
* **disk_cache:** use arcswap for now ([575b95d](https://github.com/luizfonseca/proksi/commit/575b95d9bed2e162528cc2a72394b38a06c17f42))


### Miscellaneous Chores

* release marker for 0.3.20 ([1b7be63](https://github.com/luizfonseca/proksi/commit/1b7be63154a4f559b0b57cfc664f8a81897f15e2))

## [0.3.15](https://github.com/luizfonseca/proksi/compare/v0.3.14...v0.3.15) (2024-06-22)


### Bug Fixes

* convert header function ([8d24da7](https://github.com/luizfonseca/proksi/commit/8d24da706cf1d81f3f9ca1e686b1ef137ec41789))

## [0.3.14](https://github.com/luizfonseca/proksi/compare/v0.3.13...v0.3.14) (2024-06-21)


### Bug Fixes

* **cache:** incorrect buffer size on read_body ([6cb3f2f](https://github.com/luizfonseca/proksi/commit/6cb3f2ff579ff05b9d74e5b521d3653210353ad1))

## [0.3.13](https://github.com/luizfonseca/proksi/compare/v0.3.12...v0.3.13) (2024-06-21)


### Bug Fixes

* **cache:** restore the correct status code from metadata ([fa18301](https://github.com/luizfonseca/proksi/commit/fa18301865c62008d5d5758b4757cc52b26f412c))
* **config:** make cache path optional ([01012f7](https://github.com/luizfonseca/proksi/commit/01012f7aa7adae485f3228d7dd9e5bb1e4f7cc68))
* **http_peer:** add sane defaults to peerOpts ([e9345c2](https://github.com/luizfonseca/proksi/commit/e9345c24efb1f17bf3d7ae06407f0f22f7bb6b01))

## [0.3.12](https://github.com/luizfonseca/proksi/compare/v0.3.11...v0.3.12) (2024-06-21)


### Bug Fixes

* **cache:** add mem_cache as the main default option ([4029ac0](https://github.com/luizfonseca/proksi/commit/4029ac097873d99add0ff85fbbf1d2f7cfc7f643))

## [0.3.11](https://github.com/luizfonseca/proksi/compare/v0.3.10...v0.3.11) (2024-06-20)


### Bug Fixes

* **cache:** improve writing to cache (disk) ([cef089f](https://github.com/luizfonseca/proksi/commit/cef089f4974b936f01dc71b6e89975f10d20c4f0))
* clippy ([00f5620](https://github.com/luizfonseca/proksi/commit/00f5620ee42d862692498437486f57c39de17e10))
* disable SNI callback restriction ([45f4287](https://github.com/luizfonseca/proksi/commit/45f4287ea6aa20de6c541811b2f663653377590e))

## [0.3.10](https://github.com/luizfonseca/proksi/compare/v0.3.9...v0.3.10) (2024-06-20)


### Bug Fixes

* **cache:** introduce static arc for cache paths and fix file size for cached files ([3cc4190](https://github.com/luizfonseca/proksi/commit/3cc41907fed46cc4ad23768671ebfefc90fbda21))
* **cache:** set max_cache default (without it, it's 16Kb) ([1492ce3](https://github.com/luizfonseca/proksi/commit/1492ce33fa7813690b766cbf0cf1a1c3e04686cb))

## [0.3.9](https://github.com/luizfonseca/proksi/compare/v0.3.8...v0.3.9) (2024-06-20)


### Bug Fixes

* **cache:** remove hardcoded tmp folder and .unwrap usage ([254cb05](https://github.com/luizfonseca/proksi/commit/254cb0591cdf2261eaea5274963af9f08f4018d8))

## [0.3.8](https://github.com/luizfonseca/proksi/compare/v0.3.7...v0.3.8) (2024-06-20)


### Features

* **cache:** add cache status to response headers and support expiration config ([cff30e7](https://github.com/luizfonseca/proksi/commit/cff30e7a861df3137f982fe5e237bd68afa1ec13))
* **cache:** add DiskCache storage type ([8ab5352](https://github.com/luizfonseca/proksi/commit/8ab53527a2cacaa204b88c73bd1497403fc82f53))
* **cache:** add expiration/stale config ([35c92c3](https://github.com/luizfonseca/proksi/commit/35c92c3f8724ae04ae5960f4a02355542851a61e))
* **config:** make cache configurable ([df7749f](https://github.com/luizfonseca/proksi/commit/df7749fd1f61deac737336558ad2f9dcf24adb34))


### Bug Fixes

* **cache:** generate namespaced cache keys ([2981577](https://github.com/luizfonseca/proksi/commit/29815773a8cc6de9eeccb5aa88af1f4ca4a85421))


### Miscellaneous Chores

* release marker for 0.3.9 ([ffec88a](https://github.com/luizfonseca/proksi/commit/ffec88a3d702734b13fd8786331b08c25f992d7f))

## [0.3.7](https://github.com/luizfonseca/proksi/compare/v0.3.6...v0.3.7) (2024-06-19)


### Bug Fixes

* **config:** add -d (background) mode support ([1dd6579](https://github.com/luizfonseca/proksi/commit/1dd6579220bf503e29322ff74ec4b9ec200a9cb5))

## [0.3.6](https://github.com/luizfonseca/proksi/compare/v0.3.5...v0.3.6) (2024-06-19)


### Bug Fixes

* dockerfile missing glibc ([36bba29](https://github.com/luizfonseca/proksi/commit/36bba2997f3e8375c8235eb8941a543557cbf3c3))

## [0.3.5](https://github.com/luizfonseca/proksi/compare/v0.3.1...v0.3.5) (2024-06-19)


### Features

* **config:** add support for HCL as a main configuration language ([74f9621](https://github.com/luizfonseca/proksi/commit/74f9621da9325af24e1bab87bfefd954cf3334ae))
* **hcl:** add support for HCL functions ([31c5f29](https://github.com/luizfonseca/proksi/commit/31c5f29a89467805e0e7d07d30d1c7ee60ec5dc6))


### Bug Fixes

* **hcl:** conditional check for hcl extension ([bb1d8a8](https://github.com/luizfonseca/proksi/commit/bb1d8a807bf71ff237277dd068b83b9970016aa1))


### Miscellaneous Chores

* release marker for 0.3.5 ([ace8317](https://github.com/luizfonseca/proksi/commit/ace8317cd8931832175e2d8aaf3922854c3495be))

## [0.3.1](https://github.com/luizfonseca/proksi/compare/v0.3.0...v0.3.1) (2024-06-17)


### Bug Fixes

* **docker:** transform domain names to valid ips ([3b9e843](https://github.com/luizfonseca/proksi/commit/3b9e8436ab88d64ecefe53b05861ed501201ac88))

## [0.3.0](https://github.com/luizfonseca/proksi/compare/v0.2.14...v0.3.0) (2024-06-17)


### Features

* **config:** allow passing ssl.min_proto and max_proto. Ref [#38](https://github.com/luizfonseca/proksi/issues/38) ([d90538d](https://github.com/luizfonseca/proksi/commit/d90538dc54f5025a5ccf803b60f3199ed1a1d19a))
* **config:** enable providing cert/key path to host route. Ref [#38](https://github.com/luizfonseca/proksi/issues/38) ([0ba44ba](https://github.com/luizfonseca/proksi/commit/0ba44bacf2d2aa16bffb780b9d26ce15387c7cb5))
* **config:** enable setting the logging.format ([ea68e53](https://github.com/luizfonseca/proksi/commit/ea68e53c33a7888332bfe69f48d63a4443b896c1))
* **route:** load certificates from config file. Ref [#38](https://github.com/luizfonseca/proksi/issues/38) ([4626f4f](https://github.com/luizfonseca/proksi/commit/4626f4f841f5cf985f718c6d15c96b9dbf84ac21))
* **route:** store metadata about upstreams when routing ([ab6acc4](https://github.com/luizfonseca/proksi/commit/ab6acc4a7c92b97680c6222e1afbd13c8770ca9b))

## [0.2.14](https://github.com/luizfonseca/proksi/compare/v0.2.13...v0.2.14) (2024-06-15)


### Bug Fixes

* remove unnecessary clones from arcswap stores ([9de6d98](https://github.com/luizfonseca/proksi/commit/9de6d984efb054c93c113773049e12677226acdb))

## [0.2.13](https://github.com/luizfonseca/proksi/compare/v0.2.12...v0.2.13) (2024-06-14)


### Features

* **logger:** allow enabling/disabling access/error/logs altogether ([615285d](https://github.com/luizfonseca/proksi/commit/615285d6be3a8f1541fafe70d57dcff5b210e031))


### Bug Fixes

* **docker:** container (not swarm) support for extra labels ([714d9be](https://github.com/luizfonseca/proksi/commit/714d9be17755c9ccde783541ba0bbd84bcb413b0))


### Miscellaneous Chores

* release marker for 0.2.13 ([7aac672](https://github.com/luizfonseca/proksi/commit/7aac6727f643ab04e72044f4722ba133e80a3d43))

## [0.2.12](https://github.com/luizfonseca/proksi/compare/v0.2.10...v0.2.12) (2024-06-13)


### Features

* **plugin:** add basic auth plugin ([8a4cf9e](https://github.com/luizfonseca/proksi/commit/8a4cf9e287159eebe4f4725415276dd64be89ff9))


### Bug Fixes

* **health_check:** remove unneeded temp. map ([04158f2](https://github.com/luizfonseca/proksi/commit/04158f2865bb72f8d4d98d531e5e538eb1e8cf99))
* **lets_encrypt:** fix panic caused by missing certificate directory ([691a45a](https://github.com/luizfonseca/proksi/commit/691a45ad3f30e4af9db3d6e3eb739765abd6d32a))
* **oauth2:** no-store generate state based on timestamp/url ([4a9645f](https://github.com/luizfonseca/proksi/commit/4a9645f179ea386b8d2cd1ae33ff2c424ac24cd3))
* **oauth2:** username support for filtering ([19dab94](https://github.com/luizfonseca/proksi/commit/19dab94498ebabf4188ca3ba2ca942ec6f06403f))


### Miscellaneous Chores

* release marker for 0.2.12 ([3d88b27](https://github.com/luizfonseca/proksi/commit/3d88b27c33e55678b2353f26490ce7e3d64f959b))

## [0.2.10](https://github.com/luizfonseca/proksi/compare/v0.2.9...v0.2.10) (2024-06-12)


### Bug Fixes

* **oauth2:** use .get_all to retrieve all cookie values ([cade869](https://github.com/luizfonseca/proksi/commit/cade869e2bd093d07297f4360cc58c0c20ed706b))
* **store:** use arcswap instead of dashmaps ([ad8049c](https://github.com/luizfonseca/proksi/commit/ad8049cd2db5ebc610e8266f2db80f565e765566))

## [0.2.9](https://github.com/luizfonseca/proksi/compare/v0.2.8...v0.2.9) (2024-06-12)


### Bug Fixes

* **logger:** improvements to memory usage ([fae9d4d](https://github.com/luizfonseca/proksi/commit/fae9d4d9274dfb75ce18e752dff8580386aab9ec))
* reuse dashmap contents on request ([4ee34bd](https://github.com/luizfonseca/proksi/commit/4ee34bdb5afe2869bc5f6b918e2ecf68dddd4a11))
* small type fixes ([cff726a](https://github.com/luizfonseca/proksi/commit/cff726a5991e1387673017ab00f8ba2a04e461b3))

## [0.2.8](https://github.com/luizfonseca/proksi/compare/v0.2.7...v0.2.8) (2024-06-11)


### Bug Fixes

* **oauth2:** cache policies for redirects ([77de991](https://github.com/luizfonseca/proksi/commit/77de991a802c11c091b0cc72a062a09782b441d8))
* **oauth2:** redirection of empty cookie ([647bf7c](https://github.com/luizfonseca/proksi/commit/647bf7c519ff59489f1d89e0fe2471cd9fa1894f))

## [0.2.7](https://github.com/luizfonseca/proksi/compare/v0.2.6...v0.2.7) (2024-06-10)


### Bug Fixes

* improvements to memory and cpu usage of background services ([750de40](https://github.com/luizfonseca/proksi/commit/750de403284e341c5d2fbfd0031264c768374c0a))

## [0.2.6](https://github.com/luizfonseca/proksi/compare/v0.2.5...v0.2.6) (2024-06-10)


### Bug Fixes

* **docker:** small improvements to ownership ([32824ff](https://github.com/luizfonseca/proksi/commit/32824ff6f24f1422976fd3e29b3d8f9465088258))
* use x509 and Pkey directly for certs and minor improvements ([b020bdd](https://github.com/luizfonseca/proksi/commit/b020bdd3a9296733a12060bdcd861f4998fe13d7))

## [0.2.5](https://github.com/luizfonseca/proksi/compare/v0.2.3...v0.2.5) (2024-06-10)


### Features

* **logging:** enable basic accesslogs (non-configurable) ([63452cf](https://github.com/luizfonseca/proksi/commit/63452cf58c63a4ebbb0514028e289d0f5afa096a))
* **request_id:** add request_id plugin ([6b84a47](https://github.com/luizfonseca/proksi/commit/6b84a47d5ad0b18f0f0557344edd9c55bdf8e7ba))


### Miscellaneous Chores

* release marker for 0.2.5 ([fd3411f](https://github.com/luizfonseca/proksi/commit/fd3411fba644637ed56ae2df64f266a651a51e74))

## [0.2.3](https://github.com/luizfonseca/proksi/compare/v0.2.2...v0.2.3) (2024-06-08)


### Bug Fixes

* **oauth2:** redirect to saved url from state and store cookie for top domains ([2295d1b](https://github.com/luizfonseca/proksi/commit/2295d1bfac40e9fce3e694f84d632805428af0ce))
* **routing:** converting DNS names to socketAddr now compares new and old backends correctly ([147db8b](https://github.com/luizfonseca/proksi/commit/147db8bf22109c0e732c2221a701541b095fc8bc))
* **routing:** discovery service now skips routes without upstream updates ([84b07fd](https://github.com/luizfonseca/proksi/commit/84b07fd34c9deaec0aadc55caf4e44452746ceb6))

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
