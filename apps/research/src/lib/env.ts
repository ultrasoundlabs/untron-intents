import { z } from "zod";

export function parseEnv<T>(schema: z.ZodType<T>): T {
  return schema.parse(process.env);
}
