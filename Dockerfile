# syntax=docker/dockerfile:1

FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
COPY templates templates
COPY assets assets
RUN cargo build --release --locked && mkdir /data-empty

FROM scratch
COPY --from=build /src/target/release/mewiki /mewiki
COPY --from=build --chown=1000:1000 /data-empty /data
ENV MEWIKI_DATA=/data MEWIKI_ADDR=0.0.0.0:8080
EXPOSE 8080
VOLUME /data
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD ["/mewiki", "health"]
ENTRYPOINT ["/mewiki"]
