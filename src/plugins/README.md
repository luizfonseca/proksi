# Plugin interface

The plugin interface is designed to be simple and easy to use. It allows you to extend Proksi with new features and functionality.

## Plugin types

There are two facets of plugins:

- **Middleware**: These plugins are executed before the request is sent to the upstream server. They can modify the request or response, add or remove headers, or perform other actions.

- **Extension**: These plugins are executed after the request is sent to the upstream server. They can perform additional actions, such as modifying the response or performing custom logic.

## Plugin lifecycle

When the plugin is added to Proksi, it can choose to execute in one of two phases:

`request_filter`: This phase is executed before the request is sent to the upstream server. It can modify the request or response, add or remove headers, or perform other actions (or block the request entirely).

`response_filter`: This phase is executed after the request is returned from upstream server. It can perform additional actions, such as modifying the response, adding headers or performing custom logic.

Other actions will be added in the future.

## Plugin configuration

Plugins can be configured in the Proksi configuration file. The configuration file specifies the name of the plugin, its configuration options, and any other settings that are required for the plugin to function.

Here's an example of a plugin configuration in the Proksi configuration file:

```yaml

# You can define a reference to a plugin in the configuration file
my_github_oauth_plugin: &my_oauth_plugin
  name: oauth2
  config:
    provider: github
    client_id: your-client-id
    client_secret: your-client-secret

another_oauth_plugin: &another_oauth_plugin
  name: oauth2
  config:
    provider: workos
    client_id: your-client-id
    client_secret: your-client-secret

routes:
  - host: "one.example.com"
    plugins:
      - *my_oauth_plugin

  - host: "two.example.com"
    plugins:
      - *my_github_oauth_plugin # You can reuse the plugin reference anywhere
```

In this example, the plugin is named "oauth2" and it is configured with the following options:

- `provider`: The name of the OAuth provider (e.g., "github").
- `client_id`: The client ID provided by the OAuth provider.
- `client_secret`: The client secret provided by the OAuth provider.

Note that the `config` key is optional and can be omitted if the plugin does not require any configuration options.


## Plugin API

The plugin API is designed to be simple and easy to use. It allows you to extend Proksi with new features and functionality.

Here's an example of how to use the plugin API in your plugin:

```rust
struct MyPlugin {
    your_data: String,
}


impl MiddlewarePlugin for MyPlugin {
    async fn request_filter(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        // Perform your custom logic here
        // ...

        // Do something with the session

        // Return true to stop the request here (e.g. you already returned a response)
        // Return false to continue processing the request
        Ok(true)
    }

    async fn response_filter(&self, session: &mut Session, ctx: &mut RouterContext) -> Result<bool> {
        // Perform your custom logic here
        // ...

        // Do something with the session


        // Return true to stop the request here (e.g. you already returned a response)
        // Return false to continue processing the request
        Ok(false)
    }
}
```
