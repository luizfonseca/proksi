# Changelog

## [0.17.2](https://github.com/luizfonseca/proksi/compare/proksi_plugins-v0.17.1...proksi_plugins-v0.17.2) (2026-10-25)


### Features

* let clients burst over rate limits ([#573](https://github.com/luizfonseca/proksi/issues/573)) ([1b580f0](https://github.com/luizfonseca/proksi/commit/1b580f05f7284fc2ac2e96ec80f2656a8e2163f0))
* share rate limits between replicas through redis ([#574](https://github.com/luizfonseca/proksi/issues/574)) ([bb393cf](https://github.com/luizfonseca/proksi/commit/bb393cfbde0c55213c4e20b152d5e42247227c6f))


### Miscellaneous Chores

* release 0.17.2 ([de6055b](https://github.com/luizfonseca/proksi/commit/de6055bee0e17149893cc2167d7bf9b1bbfb2399))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi_store bumped from 0.17.1 to 0.17.2

## [0.17.1](https://github.com/luizfonseca/proksi/compare/proksi_plugins-v0.17.0...proksi_plugins-v0.17.1) (2026-10-24)


### Bug Fixes

* use aws-lc-rs instead of ring, and jsonwebtoken's aws-lc backend instead of the rsa crate ([#569](https://github.com/luizfonseca/proksi/issues/569)) ([e8b15a5](https://github.com/luizfonseca/proksi/commit/e8b15a5a57254f6d86ab016cc409b07e2679d873))


### Miscellaneous Chores

* release 0.17.1 ([880169a](https://github.com/luizfonseca/proksi/commit/880169aa88f2272fdde0ea28fffe1e63fd5e1aa0))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi_store bumped from 0.17.0 to 0.17.1

## [0.17.0](https://github.com/luizfonseca/proksi/compare/proksi_plugins-v0.16.1...proksi_plugins-v0.17.0) (2026-10-22)


### Features

* add body_rewrite to change request bodies as they stream ([#556](https://github.com/luizfonseca/proksi/issues/556)) ([a87001a](https://github.com/luizfonseca/proksi/commit/a87001a03408bc3663b77788e7cfaa08cdd418a9))
* create bandwidth_limit plugin to help pace responses per client ([#550](https://github.com/luizfonseca/proksi/issues/550)) ([13c63f4](https://github.com/luizfonseca/proksi/commit/13c63f4d924679509a06bfd7abea03c804a272e4))
* create openapi plugin for api validation via specs  ([#557](https://github.com/luizfonseca/proksi/issues/557)) ([1cb1a0f](https://github.com/luizfonseca/proksi/commit/1cb1a0f947392074e2b40f17a73fb2720e6f2463))
* create waiting_room plugin for visitor-type-based locations ([#552](https://github.com/luizfonseca/proksi/issues/552)) ([7691625](https://github.com/luizfonseca/proksi/commit/76916251d9afd4b80cc13efacb261eade67e4812))


### Performance Improvements

* find the client once per request for rules, and read headers by prebuilt names ([#555](https://github.com/luizfonseca/proksi/issues/555)) ([650bf0c](https://github.com/luizfonseca/proksi/commit/650bf0cfece90ab348236a375f14888ff2487667))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * proksi_store bumped from 0.16.1 to 0.17.0
