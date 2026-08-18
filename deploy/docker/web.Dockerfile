FROM node:24-alpine AS builder

WORKDIR /workspace
RUN corepack enable

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json ./apps/web/package.json
RUN pnpm install --frozen-lockfile

COPY apps/web ./apps/web
RUN pnpm build:web \
    && touch /workspace/apps/web/dist/runtime-config.js

FROM nginxinc/nginx-unprivileged:1.28-alpine AS runtime

ARG NGINX_CONFIG=deploy/docker/nginx.conf
COPY ${NGINX_CONFIG} /etc/nginx/conf.d/default.conf
COPY --chown=101:101 --from=builder /workspace/apps/web/dist /opt/agentx-web
COPY --chmod=755 deploy/docker/web-entrypoint.d/40-runtime-config.sh /docker-entrypoint.d/40-runtime-config.sh

EXPOSE 8080
