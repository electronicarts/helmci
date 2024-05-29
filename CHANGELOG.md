# Changelog

## [1.0.0](https://github.com/electronicarts/helmci/compare/v0.5.1...v1.0.0) (2024-05-29)


### ⚠ BREAKING CHANGES

* Command line is different; probably shouldn't affect anything other then my scripts.

### Features

* Refactor release filter command line ([74a0c48](https://github.com/electronicarts/helmci/commit/74a0c4845ade1e8caa799cc16642160e74242e18))


### Bug Fixes

* Remove legacy HelmIter ([4f87759](https://github.com/electronicarts/helmci/commit/4f877591669d4d82ad55b03e2ff394d7523a9bab))

## [0.5.1](https://github.com/electronicarts/helmci/compare/v0.5.0...v0.5.1) (2024-04-28)


### Bug Fixes

* Outdated command requires helm repos to be initialized ([09c5c83](https://github.com/electronicarts/helmci/commit/09c5c83f0e74538fd78603a9cc74cdb9390c1723))

## [0.5.0](https://github.com/electronicarts/helmci/compare/v0.4.3...v0.5.0) (2024-04-26)


### Features

* Add support for shared values files. ([025b99b](https://github.com/electronicarts/helmci/commit/025b99bcf302f12cf54c0c8c3c67daa97140d520))
* Support writing config changes ([b500e6b](https://github.com/electronicarts/helmci/commit/b500e6b6398235c3763836a10eb8e449f5da07b2))


### Bug Fixes

* Delete legacy chart_version config value ([007c56a](https://github.com/electronicarts/helmci/commit/007c56a252716449b2529c253b3c0a7f9e0967f5))
* Increase max slack retries to 8 after rate limit error ([a7f8258](https://github.com/electronicarts/helmci/commit/a7f8258e2a7ce82c25d97644c8f1cc58bda8a7d6))
* Run cargo fmt ([8cb8898](https://github.com/electronicarts/helmci/commit/8cb8898fa17b77d8cdd5ff03439d4b71aefd5f61))

## [0.4.3](https://github.com/electronicarts/helmci/compare/v0.4.2...v0.4.3) (2024-04-10)


### Bug Fixes

* Really remove openssl dependancy ([ad681b2](https://github.com/electronicarts/helmci/commit/ad681b25e10def93145fbfdc884ee5450625ecfa))

## [0.4.2](https://github.com/electronicarts/helmci/compare/v0.4.1...v0.4.2) (2024-04-10)


### Bug Fixes

* Remove dependancy on openssl ([967e0d2](https://github.com/electronicarts/helmci/commit/967e0d2665da6755417e6fa541b1de8293295c0b))

## [0.4.1](https://github.com/electronicarts/helmci/compare/v0.4.0...v0.4.1) (2024-04-10)


### Bug Fixes

* Add kludge if version is not symantic version ([3d95e07](https://github.com/electronicarts/helmci/commit/3d95e070b69112aeefb94f01dc35b11dd8d7d327))
* Correct clippy warnings ([f77e45f](https://github.com/electronicarts/helmci/commit/f77e45ff42978ea390c2e2e10d8972981e4e0d07))
* Correct code formatting ([ea83ce2](https://github.com/electronicarts/helmci/commit/ea83ce263d65f3b384db523d614eb3bbfa947c0d))
* Don't display errors when legacy versions cannot be parsed ([3227f96](https://github.com/electronicarts/helmci/commit/3227f96481d8fab2f47d365c9dd6baf7a4fb7cf9))
* Refactor Outdated command ([663d4e8](https://github.com/electronicarts/helmci/commit/663d4e815e35e1ad8b0a66aae037ff8bc2ad01b6))

## [0.4.0](https://github.com/electronicarts/helmci/compare/v0.3.3...v0.4.0) (2024-04-08)


### Features

* When we skip an env or cluster then ignore config ([6f7a161](https://github.com/electronicarts/helmci/commit/6f7a161e2df4aad0c6f83ea4fcfe21604861c20b))

## [0.3.3](https://github.com/electronicarts/helmci/compare/v0.3.2...v0.3.3) (2023-11-09)


### Bug Fixes

* Ensure we set the correct registry and region when calling describe-images ([785a355](https://github.com/electronicarts/helmci/commit/785a3553e6bfbc133a699bab933fe9004ed9427e))

## [0.3.2](https://github.com/electronicarts/helmci/compare/v0.3.1...v0.3.2) (2023-11-06)


### Bug Fixes

* clippy errors in CI ([8c53d92](https://github.com/electronicarts/helmci/commit/8c53d9276ba971df5e425a434004090cbc171f49))
* Rename --cinc parameter to --cluster ([164e3cf](https://github.com/electronicarts/helmci/commit/164e3cf2126ba6eac237ca9e5c1e5d677e4e6890))

## [0.3.1](https://github.com/electronicarts/helmci/compare/v0.3.0...v0.3.1) (2023-08-09)


### Bug Fixes

* Fix incorrect slack messages ([0a20c40](https://github.com/electronicarts/helmci/commit/0a20c40db49d6cb9d9d540b54c720fd9d171c6d7))
* Refactor slack rate limit retrying ([b958843](https://github.com/electronicarts/helmci/commit/b958843805f42fd79ab5acd16802a2d76fd03029))

## [0.3.0](https://github.com/electronicarts/helmci/compare/v0.2.1...v0.3.0) (2023-07-31)


### Features

* Add basic helm-secrets support ([a2e687f](https://github.com/electronicarts/helmci/commit/a2e687f170488ee045f504d4c7345736988a7768))


### Bug Fixes

* Add RateLimitError support for helm results too ([12c2751](https://github.com/electronicarts/helmci/commit/12c2751f944308f7de45b2d8d8aa434e4e8b266a))
* Loop slack update on RateLimitError ([2bd76d1](https://github.com/electronicarts/helmci/commit/2bd76d14d0c128ffd474f1e838ac3a4bccd17081))
* Update slack code to check for rate limiting ([a50cd2f](https://github.com/electronicarts/helmci/commit/a50cd2fba05d7e589b2a54f10707b61157c22055))
* Use OsString type for Commands ([a34f9c4](https://github.com/electronicarts/helmci/commit/a34f9c48e95fa4cc766bfd01575632491038d66c))

## [0.2.1](https://github.com/electronicarts/helmci/compare/v0.2.0...v0.2.1) (2023-02-02)


### Bug Fixes

* Use Arc instead of clone for large shared objects ([a93cfbc](https://github.com/electronicarts/helmci/commit/a93cfbc3173f8342db806d1f5e88f902fcfc530e))

## [0.2.0](https://github.com/electronicarts/helmci/compare/v0.1.4...v0.2.0) (2023-01-27)


### Features

* Slack outputs full error messages from helm ([e1929a6](https://github.com/electronicarts/helmci/commit/e1929a614a788b9bd022a8cfbec304b331e4e664))


### Bug Fixes

* Clippy warning under rust 1.67 ([9585788](https://github.com/electronicarts/helmci/commit/958578895e251425077ac88207ebb1fdacea33f0))

## [0.1.4](https://github.com/electronicarts/helmci/compare/v0.1.3...v0.1.4) (2023-01-26)


### Bug Fixes

* Don't create draft release ([aa13021](https://github.com/electronicarts/helmci/commit/aa13021dc1545921a4e71a84530ed92994f9d82e))
* Simplify text message loop ([e6a541a](https://github.com/electronicarts/helmci/commit/e6a541a45d3aedecd8dda255cadc6d180e782537))

## [0.1.3](https://github.com/electronicarts/helmci/compare/v0.1.2...v0.1.3) (2023-01-25)


### Bug Fixes

* Refactor text/slack outputs ([5428fe8](https://github.com/electronicarts/helmci/commit/5428fe8419e7834cf43d373cf5324f532f48533c))
* Spelling errors in code ([32007bc](https://github.com/electronicarts/helmci/commit/32007bc86c73ec05ee0aa03e4114117860b377b0))
* Spelling of Unfortunately ([8fbef9a](https://github.com/electronicarts/helmci/commit/8fbef9a5fb179881f32b943c25331d50e79133b3))
* Use async EventStream to read events ([03ee2d4](https://github.com/electronicarts/helmci/commit/03ee2d448d9ec9dc46a76dd6890e51b75e9db362))

## [0.1.2](https://github.com/electronicarts/helmci/compare/v0.1.1...v0.1.2) (2023-01-24)


### Bug Fixes

* Doc change to force new release ([74a6525](https://github.com/electronicarts/helmci/commit/74a6525cafeb38e7a9bd9051d76d7b88483121d9))
* refactor CI ([987b1aa](https://github.com/electronicarts/helmci/commit/987b1aa109dbd30ac591836cebd2bfa6c675ebd2))
* Release process ([af170a4](https://github.com/electronicarts/helmci/commit/af170a4feb5504f50949638446b751a7b05d4dd8))

## [0.1.1](https://github.com/electronicarts/helmci/compare/v0.1.0...v0.1.1) (2023-01-24)


### Bug Fixes

* Doc change to force new release ([74a6525](https://github.com/electronicarts/helmci/commit/74a6525cafeb38e7a9bd9051d76d7b88483121d9))
* refactor CI ([987b1aa](https://github.com/electronicarts/helmci/commit/987b1aa109dbd30ac591836cebd2bfa6c675ebd2))

## 0.1.0 (2023-01-24)


### Bug Fixes

* Doc change to force new release ([74a6525](https://github.com/electronicarts/helmci/commit/74a6525cafeb38e7a9bd9051d76d7b88483121d9))
