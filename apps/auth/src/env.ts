export interface AuthEnv {
  databaseUrl: string;
  secret: string;
  authUrl: string;
  webUrl: string;
}

export function readEnv(env: NodeJS.ProcessEnv = process.env): AuthEnv {
  return {
    databaseUrl: required(env, "DATABASE_URL"),
    secret: required(env, "BETTER_AUTH_SECRET"),
    authUrl: required(env, "AUTH_URL"),
    webUrl: required(env, "WEB_URL"),
  };
}

function required(env: NodeJS.ProcessEnv, name: string): string {
  const value = env[name];
  if (!value) throw new Error(`${name} is not set. See .env.example.`);
  return value;
}
