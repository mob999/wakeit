import { createContext, isValidElement, memo, useContext, type ComponentProps } from 'react'
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

import { ResultFileLink } from '../files/ResultFileLink'
import { resultFileLinks, type FileSource } from '../files/links'
import MermaidDiagram from '../markdown/MermaidDiagram'
const remarkPlugins = [remarkGfm, remarkMath]
const rehypePlugins = [
  rehypeRaw,
  safeHtmlStyles,
  resultFileLinks,
  [rehypeSanitize, markdownSchema],
  [rehypeKatex, { throwOnError: false, trust: false, maxExpand: 1000 }],
  [rehypeHighlight, { detect: false, plainText: ['mermaid'] }],
] satisfies NonNullable<ComponentProps<typeof Markdown>['rehypePlugins']>

const FileSourceContext = createContext<FileSource | undefined>(undefined)
// Keep renderer identities stable: replacing a renderer remounts its file links
// and discards any open preview, even when the report and source are unchanged.
const components: NonNullable<ComponentProps<typeof Markdown>['components']> = {
  a: function MarkdownLink({ href, children, node }) {
    const source = useContext(FileSourceContext)
    return typeof node?.properties.dataWakeitLocal === 'string' ? (
      <ResultFileLink href={node.properties.dataWakeitLocal} source={source}>
        {children}
      </ResultFileLink>
    ) : href ? (
      <a href={href} target="_blank" rel="noopener noreferrer">
        {children}
      </a>
    ) : (
      <span>{children}</span>
    )
  },
  table: ({ children }) => (
    <div className="markdown-table">
      <table>{children}</table>
    </div>
  ),
  img: ({ src, alt, node }) =>
    typeof node?.properties.dataWakeitLocal === 'string' ? (
      <span
        title={node.properties.dataWakeitLocal}
        data-result-image-href={node.properties.dataWakeitLocal}
      >
        {alt || node.properties.dataWakeitLocal}
      </span>
    ) : src ? (
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
}

export const MarkdownContent = memo(function MarkdownContent({
  text,
  source,
}: {
  text: string
  source?: FileSource
}) {
  return (
    <FileSourceContext.Provider value={source}>
      <Markdown remarkPlugins={remarkPlugins} rehypePlugins={rehypePlugins} components={components}>
        {text.replace(/\x1b\[[0-9;]*m/g, '')}
      </Markdown>
    </FileSourceContext.Provider>
  )
})
