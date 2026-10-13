import { isValidElement, memo, type ComponentProps } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import rehypeRaw from 'rehype-raw'
import rehypeSanitize from 'rehype-sanitize'
import rehypeKatex from 'rehype-katex'
import rehypeHighlight from 'rehype-highlight'
import { markdownSchema, safeHtmlStyles } from '../markdown/safeHtml'
import 'katex/dist/katex.min.css'
import '../markdown/markdown.css'

import MermaidDiagram from '../markdown/MermaidDiagram'
const remarkPlugins = [remarkGfm, remarkMath]
const rehypePlugins = [
  rehypeRaw,
  safeHtmlStyles,
  [rehypeSanitize, markdownSchema],
  [rehypeKatex, { throwOnError: false, trust: false, maxExpand: 1000 }],
  [rehypeHighlight, { detect: false, plainText: ['mermaid'] }],
] satisfies NonNullable<ComponentProps<typeof Markdown>['rehypePlugins']>

export const MarkdownContent = memo(function MarkdownContent({ text }: { text: string }) {
  return (
    <Markdown
      remarkPlugins={remarkPlugins}
      rehypePlugins={rehypePlugins}
      components={{
        a: ({ href, children }) =>
          href ? (
            <a href={href} target="_blank" rel="noopener noreferrer">
              {children}
            </a>
          ) : (
            <span>{children}</span>
          ),
        table: ({ children }) => (
          <div className="markdown-table">
            <table>{children}</table>
          </div>
        ),
        img: ({ src, alt }) =>
          src ? (
            <img src={src} alt={alt ?? ''} loading="lazy" referrerPolicy="no-referrer" />
          ) : (
            <span>{alt}</span>
          ),
        pre: ({ children, ...props }) => {
          if (
            isValidElement<{ className?: string; children?: string }>(children) &&
            children.props.className?.split(' ').includes('language-mermaid')
          ) {
            const source = String(children.props.children ?? '').replace(/\n$/, '')
            return <MermaidDiagram source={source} />
          }
          // The HAST node is parser metadata, not a DOM attribute.
          const { node: _, ...attributes } = props
          return <pre {...attributes}>{children}</pre>
        },
      }}
    >
      {text.replace(/\x1b\[[0-9;]*m/g, '')}
    </Markdown>
  )
})
