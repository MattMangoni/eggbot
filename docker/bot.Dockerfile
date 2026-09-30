# The machine each eggbot bot lives in. Built by eggbot on first use (tag = hash of this file).
FROM node:22-bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends git ripgrep curl ca-certificates sudo procps \
 && rm -rf /var/lib/apt/lists/*

# non-root: bypassPermissions refuses to run as root; sudo lets the bot install what it needs
RUN useradd -m -s /bin/bash bot \
 && echo 'bot ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/bot \
 && mkdir /claude /work && chown bot:bot /claude /work

USER bot
RUN curl -fsSL https://claude.ai/install.sh | bash

# /claude is a shared named volume: login + session transcripts survive container rebuilds
ENV PATH=/home/bot/.local/bin:$PATH \
    CLAUDE_CONFIG_DIR=/claude \
    DISABLE_AUTOUPDATER=1
WORKDIR /work
CMD ["sleep", "infinity"]
