/**
 * Prisma client singleton.
 *
 * Next.js dev mode re-imports modules on every hot reload; without caching the
 * instance on globalThis we'd exhaust the database connection pool. In
 * production we just create one client.
 */
import { PrismaClient } from "@prisma/client";

const globalForPrisma = globalThis as unknown as { prisma?: PrismaClient };

export const prisma =
  globalForPrisma.prisma ?? new PrismaClient();

if (process.env.NODE_ENV !== "production") {
  globalForPrisma.prisma = prisma;
}
