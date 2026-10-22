# Changelog

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
