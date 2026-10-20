import { visit } from 'unist-util-visit'
import type { Root } from 'hast'
import { defaultSchema, type Options } from 'rehype-sanitize'

// Only color and inline text presentation may survive an authored style attribute.
// No layout, URLs, CSS variables, or arbitrary styles are passed to React.
function color(value: string) {
  const shape =
    /^(?:#[\da-f]{3,4}|#[\da-f]{6}|#[\da-f]{8}|[a-z]{1,24}|(?:rgb|hsl)a?\([\d\s.,%+/\-deg]+\))$/i
  return shape.test(value) && CSS.supports('color', value)
}
export function safeHtmlStyles() {
  return (tree: Root) => {
    visit(tree, 'element', (node) => {
      if (node.tagName !== 'span' || typeof node.properties.style !== 'string') return
      const input = node.properties.style
      delete node.properties.style
      if (input.length > 1024) return
      const styles = new Map<string, string>()
      for (const declaration of input.split(';').slice(0, 16)) {
        const colon = declaration.indexOf(':')
        if (colon < 0) continue
        const property = declaration.slice(0, colon).trim().toLowerCase()
        const value = declaration.slice(colon + 1).trim()
        const valid =
          (['color', 'background-color'].includes(property) && color(value)) ||
          (property === 'font-weight' && /^(?:normal|bold|[1-9]00)$/.test(value)) ||
          (property === 'font-style' && /^(?:normal|italic)$/.test(value)) ||
          (property === 'text-decoration' && /^(?:none|underline|line-through)$/.test(value))
        if (valid) styles.set(property, value)
      }
      if (styles.size)
        node.properties.style = Array.from(styles, ([key, value]) => `${key}:${value}`).join(';')
    })
  }
}
export const markdownSchema: Options = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    a: [...(defaultSchema.attributes?.a ?? []), 'dataWakeitLocal'],
    img: [...(defaultSchema.attributes?.img ?? []), 'dataWakeitLocal'],
    span: [...(defaultSchema.attributes?.span ?? []), 'style'],
    code: [
      ...(defaultSchema.attributes?.code ?? []),
      ['className', /^language-[\w-]+$/, 'math-inline', 'math-display'],
    ],
  },
}
