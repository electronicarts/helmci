# Changelog

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
