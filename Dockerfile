FROM scratch
COPY cloudid /cloudid
EXPOSE 8090
ENTRYPOINT ["/cloudid", "serve", "--config", "/etc/cloudid/config.toml"]
