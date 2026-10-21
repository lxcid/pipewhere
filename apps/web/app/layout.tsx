import type { Metadata } from "next";
import type { ReactNode } from "react";

import { AuthProvider } from "./auth-client";
import "./globals.css";

export const metadata: Metadata = { title: "Pipewhere" };

// AUTH_URL is read per request, so one image serves any deployment.
export const dynamic = "force-dynamic";

export default function RootLayout({ children }: { children: ReactNode }) {
  const authUrl = process.env.AUTH_URL;
  if (!authUrl) throw new Error("AUTH_URL is not set. See .env.example.");
  return (
    <html lang="en">
      <body>
        <AuthProvider authUrl={authUrl}>{children}</AuthProvider>
      </body>
    </html>
  );
}
