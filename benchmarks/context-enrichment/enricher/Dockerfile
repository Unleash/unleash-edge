# Node 22 for the built-in node:sqlite module (no external deps).
FROM node:22-slim
WORKDIR /app
COPY server.mjs .
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["node", "server.mjs"]
