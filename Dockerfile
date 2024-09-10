

FROM rust:alpine3.20 AS build
ARG APP_NAME=faster-elevation
ARG APP_NAME
WORKDIR /app
COPY ./ ./
RUN apk add --no-cache musl-dev
RUN apk add --no-cache gdal gdal-dev
ARG RUSTFLAGS='-C target-feature=-crt-static'
RUN cargo build --bin faster-elevation --release
RUN cp ./target/release/$APP_NAME /bin/faster-elevation

FROM alpine:3.20 AS final
LABEL authors="limlug"
LABEL maintainer="limlug@limlug.de"
ARG BUILD_DATE
ARG BUILD_VERSION
LABEL org.label-schema.build-date=$BUILD_DATE
LABEL org.label-schema.schema-version="1.0"
LABEL org.label-schema.name="faster-elevation/faster-elevation"
LABEL org.label-schema.description="A fork of Open Elevation written in Rust"
LABEL org.label-schema.url="https://faster-elevation.de/"
LABEL org.label-schema.vcs-url="https://github.com/limlug/faster-elevation"
LABEL org.label-schema.version=$BUILD_VERSION
ARG UID=10001
RUN apk add --no-cache libgcc gdal
COPY ./entrypoint.sh /bin/entrypoint.sh
RUN ["chmod", "+x", "/bin/entrypoint.sh"]
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    faster
USER faster
COPY --from=build /bin/faster-elevation /bin/
EXPOSE 3000
CMD ["/bin/entrypoint.sh"]