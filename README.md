# helmci

helmci is a program that takes a set of helm release values, typically from a dedicated repo, and automatically deploys them to a Kubernetes cluster.

## Quick Start

Install [kubectl](https://kubernetes.io/docs/tasks/tools/install-kubectl-linux/), setup, and ensure it works.

Install [helm](https://helm.sh/docs/intro/install/), ensure it works.

Install [helm-diff plugin](https://github.com/databus23/helm-diff):

```sh
helm plugin install https://github.com/databus23/helm-diff
```

Edit kubectl context in `example/helm-values/envs/dev/dev-cluster/config.yaml`.

There are three output methods. By default the text output is used if nothing else specified.
Add `--output text` (gitlab flavoured text), `--output slack` or `--output tui` full screen text mode to the following commands. By default `text` is used.

The `tui` full screen text mode has the following keyboard bindings:

* `q`: Request exit when all processes finished. This won't kill processes currently running.
* `ctrl-c`: Same as above.
* `l`: List installations.
* `ESC`: From installations, toggle stop requested. From commands, go to installations.
* `up`/`down`: select next/previous item.
* `enter`: From installations, show commands for the selected item.
* `pg up`/`pg down`: Scroll the details pane up/down.
* `left`/`right` scroll the details pane left/right.

Run commands such as:

```sh
cargo run -- --help
cargo run -- --vdir ./example/helm-values test
cargo run -- --vdir ./example/helm-values diff
cargo run -- --vdir ./example/helm-values template
cargo run -- --vdir ./example/helm-values outdated
```

Only do this if you really want to deploy:

```sh
cargo run -- --vdir ./example/helm-values upgrade
```

## Additional Options

### Bypass Upgrade on No Changes

The `-b` or `--bypass-upgrade-on-no-changes` option allows you to bypass the upgrade process if no changes are detected. This can be useful to save time and resources when you are confident that no changes have been made to the release values.

### Suboptions

#### Yes

The `-y` or `--yes` suboption can be used in conjunction with the `--bypass-upgrade-on-no-changes` option to automatically proceed with the upgrade even if no changes are detected. This is useful for automated scripts where manual intervention is not possible.

#### No

The `-n` or `--no` suboption can be used in conjunction with the `--bypass-upgrade-on-no-changes` option to automatically skip the upgrade if no changes are detected. This is useful when you want to ensure that upgrades are only performed when necessary without manual intervention.

Example usage:

```sh
# Bypass upgrade on no changes and automatically proceed with the upgrade
cargo run -- --vdir ./example/helm-values upgrade --bypass-upgrade-on-no-changes --yes
cargo run -- --vdir ./example/helm-values upgrade -b -y


# Bypass upgrade on no changes and automatically skip the upgrade
cargo run -- --vdir ./example/helm-values upgrade --bypass-upgrade-on-no-changes --no
cargo run -- --vdir ./example/helm-values upgrade -b -y
```

## Config layout

You can have zero or more environments.

An environment can have zero or more clusters.

A cluster can have zero or more releases, which corresponds to a helm release that gets installed.

## Environment config

```yaml
cat example/helm-values/envs/dev/config.yaml
locked: false
```

* Setting `locked` to true disables all releases under the env.

The name of the directory corresponds to the name of the environment. In the above case it will be called `dev`.

## Cluster config

For example `example/helm-values/envs/dev/dev-cluster/config.yaml`:

```yaml
context: 1234-dev-cluster
locked: false
```

* `context` is the `--kube-context` parameter passed to helm.
* Setting `locked` to true disables all releases under the cluster dir.

The name of the directory corresponds to the name of the cluster. Which can be different from the context value provided. In the above example, the cluster is called `dev-cluster` and uses the parameter `--kube-context 1234-dev-cluster`.

## Release config

For example `example/helm-values/envs/dev/dev-cluster/kube-prometheus-stack/config.yaml`:

```yaml
auto: true
locked: false
namespace: monitoring
timeout: 180
release: kube-prometheus-stack
release_chart:
  type: helm
  repo_url: https://prometheus-community.github.io/helm-charts
  chart_name: kube-prometheus-stack
  chart_version: 35.5.1
depends: []
```

* Setting `auto` to false disables deploys unless using `--auto=all` parameter.
* Setting `locked` to false disables all deploys.
* `namespace` is the kubernetes namespace to use for deploys.
* `release` is the release name for the helm chart.
* `depends` is a list of releases that must be installed first. `$namespace/$release` format.
* `release_chart` is how to find the chart to install (see below).

Note: the value of the `release` is used as the release name, not the name of the directory.

The `values.yaml` and (optionally) `values.secrets` files contain the helm values. Note `values.secrets` can be encrypted, but if so must be decrypted using whatever mechanism before running helmci.

## Override config

Used to override `release_chart` details for testing purposes. Filename provided by the `--overrides` parameter.

```yaml
releases:
  kube-prometheus-stack:
    type: local
    path: /home/me/charts/kube-prometheus-stack
```

## Helm chart config

Example helm chart specification:

```yaml
release_chart:
  type: helm
  repo_url: https://prometheus-community.github.io/helm-charts
  chart_name: kube-prometheus-stack
  chart_version: 35.5.1
```

Example OCI chart specification:

```yaml
release_chart:
  repo_url: oci://xxxx.dkr.ecr.us-east-2.amazonaws.com/helm-charts
  chart_name: sqlpad
  chart_version: 0.0.1
  type: oci
```

Example local directory specification:

```yaml
release_chart:
  kube-prometheus-stack:
    type: local
    path: /home/me/charts/kube-prometheus-stack
```

## History

There are a number of existing solutions of deploying stuff on a kubernetes cluster. The main contenders are:

* [argocd](https://argoproj.github.io/cd/)
* [spinnaker](https://spinnaker.io/)
* [harness](https://docs.harness.io/)

Unfortunately, these all had limitations:

* argocd, while it supports helm charts, it does not support saving helm values in git.
* Can overcome this issue by creating git repo of charts containing helm sub-charts, but this is ugly and requires modifying the helm values.
* spinnaker seems rather heavy weight to install, haven't succeeded yet.
* harness is mostly proprietary solution.

As a result a Python program was written as an alternative solution. This is a rewrite of the Python program.

## Current Limitations

* Running helmci upgrade on chart that has not changed results in an upgrade regardless. Which depending on the chart could be slow, and adds useless helm metadata to Kubernetes. Ideally need some way to skip charts if nothing has really changed.
* Should integrate better with secrets mechanisms that do not store plain text version in working directory, such as sops.
* Should be able to save hash of chart to ensure it is not unexpectedly changed upstream.
* No idea how well `text` will work with github, only tested with gitlab.
