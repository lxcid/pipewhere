-- Privileged setup, run as the PostgreSQL superuser before either service
-- migrates. It is safe to run again: it creates what is missing and resets
-- both roles' passwords to the configured values.

\set ON_ERROR_STOP on

SELECT format('CREATE ROLE pipewhere_auth LOGIN PASSWORD %L', :'auth_password')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'pipewhere_auth') \gexec

SELECT format('CREATE ROLE pipewhere_api LOGIN PASSWORD %L', :'api_password')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'pipewhere_api') \gexec

ALTER ROLE pipewhere_auth WITH LOGIN PASSWORD :'auth_password';
ALTER ROLE pipewhere_api WITH LOGIN PASSWORD :'api_password';
ALTER ROLE pipewhere_auth SET search_path = auth;
ALTER ROLE pipewhere_api SET search_path = pipewhere;

REVOKE ALL ON DATABASE :"DBNAME" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"DBNAME" TO pipewhere_auth, pipewhere_api;
REVOKE ALL ON SCHEMA public FROM PUBLIC;

CREATE SCHEMA IF NOT EXISTS auth AUTHORIZATION pipewhere_auth;
CREATE SCHEMA IF NOT EXISTS pipewhere AUTHORIZATION pipewhere_api;
