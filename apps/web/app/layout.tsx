import type { Metadata } from "next";
import type { ReactNode } from "react";

import { ApiProvider } from "./api-client";
import { AuthProvider } from "./auth-client";
import "./globals.css";

export const metadata: Metadata = { title: "Pipewhere" };

// AUTH_URL and API_URL are read per request, so one image serves any deployment.
export const dynamic = "force-dynamic";

export default function RootLayout({ children }: { children: ReactNode }) {
  const authUrl = process.env.AUTH_URL;
  if (!authUrl) throw new Error("AUTH_URL is not set. See .env.example.");
  const apiUrl = process.env.API_URL;
  if (!apiUrl) throw new Error("API_URL is not set. See .env.example.");
  return (
    <html lang="en">
      <body>
        <AuthProvider authUrl={authUrl}>
          <ApiProvider apiUrl={apiUrl}>{children}</ApiProvider>
        </AuthProvider>
      </body>
    </html>
  );
}
