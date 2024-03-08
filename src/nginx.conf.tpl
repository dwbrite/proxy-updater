events {}

stream {
    {{#each ports}}
    server {
        listen {{ this }};
        proxy_pass {{ ../ip }}:{{ this }};
    }
    {{/each}}
}
