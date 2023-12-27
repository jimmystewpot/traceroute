FROM golang:1.21-bookworm AS BuildStage

WORKDIR /usr/src/traceroute

COPY . .

RUN go mod download

RUN go build -o /usr/local/bin/traceroute main.go

FROM ubuntu:jammy

WORKDIR /

COPY --from=BuildStage /usr/local/bin/traceroute /traceroute

ENTRYPOINT [ "/traceroute udp" ]