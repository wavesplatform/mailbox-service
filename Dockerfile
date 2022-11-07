FROM rust:1.63 as builder
WORKDIR /usr/src/service

RUN rustup component add rustfmt

COPY Cargo.* ./
COPY ./src ./src

RUN cargo install --path .


FROM debian:11 as runtime
WORKDIR /app

RUN apt-get update && apt-get install -y curl openssl libssl-dev libpq-dev procps net-tools curl
RUN /usr/sbin/update-ca-certificates

COPY --from=builder /usr/local/cargo/bin/maillbox-server .

CMD ["./maillbox-server"]
