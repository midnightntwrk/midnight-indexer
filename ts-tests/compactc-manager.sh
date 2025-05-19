#!/bin/sh
VERSION=v0.21.0
DOCKER_IMAGE=ghcr.io/midnight-ntwrk/compactc:${VERSION}
RELEASE=https://api.github.com/repos/midnight-ntwrk/artifacts/releases/tags/compactc-${VERSION}

cd "$(dirname "$0")"
COMPACT_HOME_LOCAL="$(pwd)/managed/compactc-release"

check_os() {
    if [ "$(uname)" = "Darwin" ] && [ "$(uname -p)" = "arm" ]; then
      COMPACT_OS=macos
    elif [ "$(uname)" = "Darwin" ] && [ "$(uname -p)" = "i386" ]; then
      COMPACT_OS=docker
    elif [ "$(expr substr "$(uname -s)" 1 5)" = "Linux" ]; then
      COMPACT_OS=linux
    else
      COMPACT_OS=docker
    fi
}

check_os

if [ "$COMPACT_OS" = "docker" ]; then
  echo "Using docker..."
  docker pull $DOCKER_IMAGE
  docker run -v $PWD:/midnight $DOCKER_IMAGE "compactc /midnight/$1 /midnight/$2"
else
  echo "Using binary release..."
  if [ -n "$COMPACT_HOME" ]; then
    echo "Environment variable COMPACT_HOME is set to: $COMPACT_HOME"
  else
    echo "Environment variable COMPACT_HOME is not set. Checking possible local installation from previous execution or trying to download..."
    COMPACT_HOME=$COMPACT_HOME_LOCAL
    if [ -d "$COMPACT_HOME" ]; then
      echo "Assuming that compiler is already in $COMPACT_HOME..."
    else
      echo "Attempt to download the compactc from GitHub..."
      if [ -z "$GITHUB_TOKEN" ]; then
        echo "Environment variable GITHUB_TOKEN is not set. Can't download."
        exit 1
      else
        COMPACT_HOME=$COMPACT_HOME_LOCAL
        echo "Downloading compactc into $COMPACT_HOME..."
        echo "Using OS release: $COMPACT_OS"
        response=$(curl -H "Authorization: Bearer $GITHUB_TOKEN" $RELEASE)
        clean_response=$(echo "$response" | tr -d '\000-\031' | jq 'del(.body)')
        echo "Checking release $RELEASE ..."
        URL=$(echo "$clean_response" | jq --arg FILE "compactc-$COMPACT_OS.zip" -r '.assets[] | select(.name | test($FILE)) | .url')
        echo "Downloading release $URL..."
        mkdir managed
        curl -H "Authorization: Bearer $GITHUB_TOKEN" -H "Accept: 'application/octet-stream'" -o managed/compactc.zip -L "$URL"
        mkdir $COMPACT_HOME
        unzip managed/compactc.zip -d $COMPACT_HOME
        chmod -R +w managed
      fi
    fi
  fi

  if [ -f "$COMPACT_HOME/compactc" ]; then
    echo "Attempt to compile with params:" "$@"
    echo
    $COMPACT_HOME/compactc "$@"
  else
    echo "Can not find: $COMPACT_HOME/compactc. Try to: delete $COMPACT_HOME_LOCAL and rerun or manually download correct 'compactc' version and set COMPACT_HOME env variable."
    exit 1
  fi
fi

exit 0