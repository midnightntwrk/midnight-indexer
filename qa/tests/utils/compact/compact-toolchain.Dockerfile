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

# The compactc release to install. Tags the image.
ARG COMPACT_VERSION
# The `compact` toolchain manager release that provides `compact update`.
ARG COMPACT_MANAGER_VERSION

RUN test -n "$COMPACT_VERSION" || { echo "COMPACT_VERSION build arg is required" >&2; exit 1; }
RUN test -n "$COMPACT_MANAGER_VERSION" || { echo "COMPACT_MANAGER_VERSION build arg is required" >&2; exit 1; }

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl unzip xz-utils \
 && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.local/bin:${PATH}"

RUN curl --proto '=https' --tlsv1.2 -LsSf \
      "https://github.com/midnightntwrk/compact/releases/download/compact-v${COMPACT_MANAGER_VERSION}/compact-installer.sh" \
    | sh

RUN compact update "$COMPACT_VERSION"

WORKDIR /work
ENTRYPOINT ["compact"]
