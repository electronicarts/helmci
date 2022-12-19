FROM docker.io/library/rust:1.66-alpine3.16 as builder

RUN apk update && apk add \
    gcc \
    g++ \
    zlib \
    zlib-dev

ADD ./Cargo.toml Cargo.lock ./
RUN mkdir src \
    && touch src/lib.rs \
    && cargo build --release \
    && rm -rf src

ADD ./src ./src
RUN cargo build --release

FROM alpine:3.15
ARG APP=/app

RUN apk add --update-cache \
    shadow \
    ca-certificates \
    && rm -rf /var/cache/apk/*

EXPOSE 8000

ENV TZ=Etc/UTC \
    APP_USER=appuser

RUN groupadd $APP_USER \
    && useradd -g $APP_USER $APP_USER \
    && mkdir -p ${APP}

COPY --from=builder /target/release/helmci ${APP}/helmci

USER $APP_USER
WORKDIR ${APP}

CMD ["./helmci"]
