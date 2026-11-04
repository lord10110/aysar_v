# Aysar in a container: CLI + MCP server + dashboard, with Chromium for rendering.
# Coding agents are not included; mount your own or use dry-run.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p aysar-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends chromium ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/aysar /usr/local/bin/aysar
ENV AYSAR_CHROME=/usr/bin/chromium AYSAR_PROJECTS_DIR=/projects
VOLUME /projects
EXPOSE 7331
ENTRYPOINT ["aysar"]
CMD ["ui", "--no-open", "--port", "7331", "--bind", "0.0.0.0"]
