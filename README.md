# caseproxy
A static file server that matches paths case-insensitively.

```
Usage: caseproxy [OPTIONS]

Options:
  -p, --port <PORT>
          TCP port to listen on

  -H, --host <HOST>
          Host to listen on when using TCP
          
          [default: localhost]

  -s, --socket-path <SOCKET_PATH>
          Path to Unix socket to listen on

  -r, --root-path <ROOT_PATH>
          Root directory to serve files from
          
          [default: .]

  -u, --url-prefix <URL_PREFIX>
          A prefix that should be stripped from request URLs before resolving on-disk paths
          
          [default: /]

      --sendfile
          Whether to use `X-Sendfile` header.
          
          Signals the proxying httpd to serve the resolved file directly. Only supported by Apache and lighttpd.

      --nginx <NGINX_URL>
          
          URL prefix to use with `X-Accel-Redirect` header, which can be used to
          signal the proxying httpd to serve the resolved file directly with
          appropriate configuration. Only supported by nginx.
          
          The path on disk relative to `--root-path` will be appended to this
          value and sent to nginx triggering an internal redirect. For example,
          a value of `/files/_caseproxied/` will work with an nginx configuration like;
          ```
          location /files {
             proxy_pass ...;
             location /files/_caseproxied {
                 alias ...; # full path to `--root-path`
                 internal; # optional, location only matches when redirected via `X-Accel-Redirect`
             }
          }
          ```

  -h, --help
          Print help (see a summary with '-h')
```
