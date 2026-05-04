import { defineCollection, z } from 'astro:content';
import { docsSchema } from '@astrojs/starlight/schema';

// Extend Starlight's default frontmatter with KeiSei provenance fields
// emitted by the `keidocs` Rust primitive.
const keiseiFrontmatter = z.object({
  dna_hash: z.string().optional(),
  signed_by: z.string().optional(),
  parent: z.string().optional(),
});

export const collections = {
  docs: defineCollection({
    schema: docsSchema({ extend: keiseiFrontmatter }),
  }),
};
