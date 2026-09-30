# The machine each eggbot bot lives in. Built by eggbot on first use (tag = hash of this file).
FROM node:22-bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends git ripgrep curl ca-certificates sudo procps \
 && rm -rf /var/lib/apt/lists/*

# non-root: bypassPermissions refuses to run as root; sudo lets the bot install what it needs
RUN useradd -m -s /bin/bash bot \
 && echo 'bot ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/bot \
 && mkdir /claude /codex /work && chown bot:bot /claude /codex /work

# pinned: eggbot speaks Codex's app-server protocol, which is still marked experimental
RUN npm install -g @openai/codex@0.159.0 && npm cache clean --force

USER bot
RUN curl -fsSL https://claude.ai/install.sh | bash

# /claude and /codex are shared named volumes: logins + session transcripts survive container rebuilds
ENV PATH=/home/bot/.local/bin:$PATH \
    CLAUDE_CONFIG_DIR=/claude \
    CODEX_HOME=/codex \
    DISABLE_AUTOUPDATER=1
WORKDIR /work
CMD ["sleep", "infinity"]
