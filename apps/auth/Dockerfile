FROM node:26.10.0-slim
RUN npm install --global pnpm@11.27.1
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/auth/package.json apps/auth/
RUN pnpm install --frozen-lockfile --prod --filter @pipewhere/auth
COPY apps/auth/src apps/auth/src
WORKDIR /app/apps/auth
ENV NODE_ENV=production
USER node
EXPOSE 3001
CMD ["node", "src/server.ts"]
