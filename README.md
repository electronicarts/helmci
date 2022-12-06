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

## Environment config

```yaml
cat example/helm-values/envs/dev/config.yaml
locked: false
```

* Setting `locked` to true disables all releases under the env.

## Cluster config

For example `example/helm-values/envs/dev/dev-cluster/config.yaml`:

```yaml
context: dev-cluster
locked: false
```

* `context` is the `--kube-context` parameter passed to helm.
* Setting `locked` to true disables all releases under the cluster dir.


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
* `namespace` is the kubenetes namespace to use for deploys.
* `release` is the release name for the helm chart.
* `depends` is a list of releases that must be installed first. `$namespace/$release` format.
* `release_chart` is how to find the chart to install (see below).

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

Unfortuanately, these all had limitations.

* argocd, while it supports helm charts, it does not support saving helm values in git.
* Can overcome this issue by creating git repo of charts containing helm sub-charts, but this is ugly and requires modifying the helm values.
* spinnaker seems rather heavy weight to install, haven't succeeded yet.
* harness is mostly proprietary solution.

As a result a Python program was written as an altnerative solution. This is a rewrite of the Python program.

## Current Limitations

* Running helmci upgrade on chart that has not changed results in an upgrade regardless. Which depending on the chart could be slow.
* Should integrate better with secrets mechanisms that do not store plain text version in working directory, such as sops.
* Should be able to save hash of chart to ensure it is unexpectedly changed upstream.