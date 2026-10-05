# This file is part of midnightntwrk/midnight-indexer
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# compact-toolchain — the Compact compiler in a container, so compiling a test
# fixture needs nothing on the host but Docker. Built on first use by
# `utils/compact/compact-compiler.ts` and cached as an image afterwards.

FROM debian:stable-slim

# The compactc release to install, e.g. `0.30.0` or `0.33.0-rc.2`. Tags the image.
ARG COMPACT_VERSION
# The `compact` toolchain manager release that provides `compact update`.
ARG COMPACT_MANAGER_VERSION
# Where pre-release compilers come from; see the install step below.
ARG COMPACT_PRERELEASE_REPO=LFDT-Minokawa/compact

RUN test -n "$COMPACT_VERSION" || { echo "COMPACT_VERSION build arg is required" >&2; exit 1; }
RUN test -n "$COMPACT_MANAGER_VERSION" || { echo "COMPACT_MANAGER_VERSION build arg is required" >&2; exit 1; }

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl unzip xz-utils \
 && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.local/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -LsSf \
      "https://github.com/midnightntwrk/compact/releases/download/compact-v${COMPACT_MANAGER_VERSION}/compact-installer.sh" \
    | sh

# The toolchain manager only offers stable releases: `compact list` jumps from
# 0.31.1 straight to 0.34.0. Pre-releases are published as GitHub releases on
# the compiler repo instead, with the same archive layout the manager unpacks,
# so fall back to fetching one directly. A pinned pre-release is how a fixture
# targets a compact-runtime that no stable compiler emits yet.
RUN set -eu; \
    if compact update "$COMPACT_VERSION"; then \
      echo "compactc ${COMPACT_VERSION}: installed via the toolchain manager"; \
    else \
      case "$(uname -m)" in \
        x86_64)  target=x86_64-unknown-linux-musl ;; \
        aarch64) target=aarch64-unknown-linux-musl ;; \
        *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;; \
      esac; \
      echo "compactc ${COMPACT_VERSION}: not offered by the manager, fetching the pre-release"; \
      dest="/root/.compact/versions/${COMPACT_VERSION}/${target}"; \
      mkdir -p "$dest"; \
      curl --proto '=https' --tlsv1.2 -fLsS -o /tmp/compactc.zip \
        "https://github.com/${COMPACT_PRERELEASE_REPO}/releases/download/compactc-v${COMPACT_VERSION}/compactc_v${COMPACT_VERSION}_${target}.zip"; \
      unzip -q /tmp/compactc.zip -d "$dest"; \
      rm /tmp/compactc.zip; \
      chmod +x "$dest"/*; \
    fi

# One path to the selected compiler whichever way it arrived, so the entrypoint
# does not depend on the manager's notion of a default version.
RUN ln -s "$(dirname "$(ls /root/.compact/versions/${COMPACT_VERSION}/*/compactc)")" /opt/compactc \
 && /opt/compactc/compactc --version

WORKDIR /work
ENTRYPOINT ["/opt/compactc/compactc"]
