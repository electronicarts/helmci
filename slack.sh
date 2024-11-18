#!/usr/bin/env sh
set -e

export SLACK_URL
export SLACK_API_TOKEN
export SLACK_CHANNEL

SLACK_URL="$(pass show ea/slack/url)"
SLACK_API_TOKEN="$(pass show ea/slack/token)"
SLACK_CHANNEL="$(pass show ea/slack/token)"

exec "$@"
