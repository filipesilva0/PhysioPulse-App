FROM alpine:3.20
RUN apk add --no-cache postgresql16-client age tar coreutils rclone jq
COPY backup.sh restore.sh run-nightly.sh status.sh /backup/
RUN chmod +x /backup/*.sh
CMD ["/backup/run-nightly.sh"]
