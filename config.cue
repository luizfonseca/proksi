package config

// TYPEDEF
#ProksiMiddleware: {
  // Should be unique
  name?: string
  options?: _
}

#ProksiPathNode: {
  path?: string
  path_regex?: string
  is_prefix?: bool
  is_suffix?: bool
}

#ProksiUpstream: {
  addr: string
  port?: >1000 & <9999
  weight?: >0 & <100
}

#ProksiRoute: {
  paths?: [...#ProksiPathNode]
  host?: string
  middlewares?: [...#ProksiMiddleware]
  upstreams: [...#ProksiUpstream]
}

#ProksiAgentOptions: {
  host?: string
  port?: >1000 & <9999
  enabled?: bool
  serviceName?: string
}

// General options
#ProksiGeneral: {
  ports?: {
    http?: >1000 & <9999
    https?: >1000 & <9999
  }
  host?: string
  logLevel?: string

  tracing?: #ProksiAgentOptions
  metrics?: #ProksiAgentOptions
}

// Type definitions for the Proksi configuration file
#Proksi: {
  general?: #ProksiGeneral
  middlewares?: [...#ProksiMiddleware]
  routes: [...#ProksiRoute]
}

// Implementation
proksi: #Proksi & {
  general: {
    ports: {
      http: 8080,
      https: 8443
    },
    host: "localhost",
    logLevel: "info"
  }

  middlewares: [
    {
      name: "cors",
      options: {
        origin: "*"
      }
    }
  ]

  routes: [
    {
      host: "http://example.com",
      upstreams: [
        { addr: "10.0.1.24/24", port: 3000, weight: 2 },
        { addr: "10.0.1.26/24", port: 3000, weight: 1 },
      ]
      paths: [
        {
          path: "/api"
        }
      ],
      middlewares: [
        {
          name: "cors",
          options: {
            origin: "*"
          }
        }
      ]
    }
  ]
}
