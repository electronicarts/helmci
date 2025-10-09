# Changelog

## [2.0.9](https://github.com/electronicarts/helmci/compare/v2.0.8...v2.0.9) (2025-10-09)


### Bug Fixes

* update helm inputs ([a9ca237](https://github.com/electronicarts/helmci/commit/a9ca2370fd9deebeb98956d8e4a64efd210ac16a))
* update nix flake inputs ([9592812](https://github.com/electronicarts/helmci/commit/9592812c61becd4212d26c6600143dad5e579eed))

## [2.0.8](https://github.com/electronicarts/helmci/compare/v2.0.7...v2.0.8) (2025-07-07)


### Bug Fixes

* add lint check before upgrade ([4b95d66](https://github.com/electronicarts/helmci/commit/4b95d6659e367088ee56134615d5dcae08516852))

## [2.0.7](https://github.com/electronicarts/helmci/compare/v2.0.6...v2.0.7) (2025-07-01)


### Bug Fixes

* Simplify sending preformated text to slack ([9ce8966](https://github.com/electronicarts/helmci/commit/9ce896684511aeca371e563f9fa7be754b22b989))

## [2.0.6](https://github.com/electronicarts/helmci/compare/v2.0.5...v2.0.6) (2025-06-17)


### Bug Fixes

* correct typo in slack.sh script ([cd5b5ca](https://github.com/electronicarts/helmci/commit/cd5b5ca38e13ca576b6e739021077943a57f0f86))
* reformat slack installation block as non-table ([0bb8640](https://github.com/electronicarts/helmci/commit/0bb8640a4f07f86cb50c85b923bc962e6c5e9c39))
* Simplify --bypass_skip_upgrade_on_no_changes option ([95da67c](https://github.com/electronicarts/helmci/commit/95da67ce57bd88dfd6131b1b3e9ef16ff850f0c2))

## [2.0.5](https://github.com/electronicarts/helmci/compare/v2.0.4...v2.0.5) (2025-04-29)


### Bug Fixes

* fix workers exiting when blocked jobs ([8f32b45](https://github.com/electronicarts/helmci/commit/8f32b456ed9c98f8de2294f93a99d15bfb798b39))
* make lint operation use charts properly ([97c3673](https://github.com/electronicarts/helmci/commit/97c36736a03a38ce12d97e6002f8dc73ebf3ca60))
* update nix flake inputs ([16ac75c](https://github.com/electronicarts/helmci/commit/16ac75c1185761ba239a51a231df9b35339954d6))

## [2.0.4](https://github.com/electronicarts/helmci/compare/v2.0.3...v2.0.4) (2025-03-27)


### Bug Fixes

* don't fail outdated if AWS doesn't return version info ([516741b](https://github.com/electronicarts/helmci/commit/516741b8c28d98c60391e6dac9bef6f9853509a7))

## [2.0.3](https://github.com/electronicarts/helmci/compare/v2.0.2...v2.0.3) (2025-02-19)


### Bug Fixes

* don't process job if dependancies fail ([e6f346a](https://github.com/electronicarts/helmci/commit/e6f346a33dfb0e2303ba5901caeada8cae50a1bf))
* local repos correctly create .tgz files ([46b39eb](https://github.com/electronicarts/helmci/commit/46b39eb2436750c1b53855e0852bbdfd18e7629d))
* update nix flake inputs ([73c0c7f](https://github.com/electronicarts/helmci/commit/73c0c7f70dd72969c6095ba3bdd1031e1adfb0a8))

## [2.0.2](https://github.com/electronicarts/helmci/compare/v2.0.1...v2.0.2) (2024-11-29)


### Bug Fixes

* add .gitignore file for example ([d7410d0](https://github.com/electronicarts/helmci/commit/d7410d0397dde582265ee92677ec34cbb3667d87))
* add lock file to example ([01065a4](https://github.com/electronicarts/helmci/commit/01065a413dae7f4e478b4435b09510167b639515))
* improve update performance if no changes ([4eea8b3](https://github.com/electronicarts/helmci/commit/4eea8b336fa367ad0ecfbc25f0e1b2059996a01b))
* Remove override config from docs too ([97a5f4f](https://github.com/electronicarts/helmci/commit/97a5f4f6b6b6a060a3c9418512adb93127acbc26))
* remove overrides support ([2ca9c03](https://github.com/electronicarts/helmci/commit/2ca9c0384d389753b04c319fafc4ca40bda7cc93))
* support sops.yaml file by default ([94b63c9](https://github.com/electronicarts/helmci/commit/94b63c95cec130defe7e2d1c6e3ab1cd1954fae3))

## [2.0.1](https://github.com/electronicarts/helmci/compare/v2.0.0...v2.0.1) (2024-11-26)


### Bug Fixes

* rewrite caching API ([aa0ccce](https://github.com/electronicarts/helmci/commit/aa0ccce7935f8de8cdb9a97a4aa8518e5042fedf))

## [2.0.0](https://github.com/electronicarts/helmci/compare/v1.3.3...v2.0.0) (2024-11-20)


### ⚠ BREAKING CHANGES

* Add support for chart lock files

### Features

* Add .envrc and slack.sh files ([84000f4](https://github.com/electronicarts/helmci/commit/84000f42b8241805f6b4bcc310b818518afd423c))
* Add support for chart lock files ([c609fba](https://github.com/electronicarts/helmci/commit/c609fba81eb82c5c6d9fc5e6902cffaa28c5f178))


### Bug Fixes

* add checks to ensure requested chart name and version consistant with lock file ([642f81b](https://github.com/electronicarts/helmci/commit/642f81b6523fa8dc0fe1bb27da4995c18817dbc1))
* replace tui with forked ratatui repo ([225e3f8](https://github.com/electronicarts/helmci/commit/225e3f825a9e73fe0507ba6a7e644cfc85a69d27))

## [1.3.3](https://github.com/electronicarts/helmci/compare/v1.3.2...v1.3.3) (2024-10-18)


### Bug Fixes

* filter out useless helm messages to slack ([4f68d2f](https://github.com/electronicarts/helmci/commit/4f68d2ff4216acda82289eefc5939d0eca1d53c2))

## [1.3.2](https://github.com/electronicarts/helmci/compare/v1.3.1...v1.3.2) (2024-10-17)


### Bug Fixes

* attempt to highlight stderr output and stdout/stderr error output ([26aa21f](https://github.com/electronicarts/helmci/commit/26aa21fe84640c662d4612b7ba7d984cde0392eb))

## [1.3.1](https://github.com/electronicarts/helmci/compare/v1.3.0...v1.3.1) (2024-09-05)


### Bug Fixes

* attempt to make skipped symbol obvious under slack ([9e8d6a0](https://github.com/electronicarts/helmci/commit/9e8d6a0918701d683558143407caee8692513ccf))

## [1.3.0](https://github.com/electronicarts/helmci/compare/v1.2.1...v1.3.0) (2024-09-05)


### Features

* Show in output when upgrade skipped due no changes ([311dddf](https://github.com/electronicarts/helmci/commit/311dddfc6ed30fbebff1fc381c582008ca8f2e82))


### Bug Fixes

* address clippy warning ([cfa13cd](https://github.com/electronicarts/helmci/commit/cfa13cd143b055b01073383caaee17f21fdfa494))
* Fix clippy warning expect used ([8bbd0df](https://github.com/electronicarts/helmci/commit/8bbd0df91c008af95d1b94351e14308285622e49))

## [1.2.1](https://github.com/electronicarts/helmci/compare/v1.2.0...v1.2.1) (2024-08-30)


### Bug Fixes

* Specify default rust crypto provider ([d1d0ecd](https://github.com/electronicarts/helmci/commit/d1d0ecdc8b55ae987dc62a3702e6408c02d6a36b))

## [1.2.0](https://github.com/electronicarts/helmci/compare/v1.1.0...v1.2.0) (2024-08-30)


### Features

* Use latest macos runner ([e4084e9](https://github.com/electronicarts/helmci/commit/e4084e9e463dd12336d82d40003140a8d3d40894))

## [1.1.0](https://github.com/electronicarts/helmci/compare/v1.0.0...v1.1.0) (2024-08-29)


### Features

* skip upgrade if no changes detected ([#379](https://github.com/electronicarts/helmci/issues/379)) ([d227422](https://github.com/electronicarts/helmci/commit/d227422d4df98abb5d63cda5dec902bae451dec9))

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
