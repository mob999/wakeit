import { createContext, isValidElement, memo, useContext, type ComponentProps } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import rehypeRaw from 'rehype-raw'
import rehypeSanitize from 'rehype-sanitize'
import rehypeKatex from 'rehype-katex'
import rehypeHighlight from 'rehype-highlight'
import { markdownSchema, safeHtmlStyles } from './safeHtml'
import 'katex/dist/katex.min.css'
import './markdown.css'

import { ResultFileLink } from '../files/ResultFileLink'
import { resultFileLinks, type FileSource } from '../files/links'
import { ResultImage } from '../files/ResultImage'
import type { ImageSource } from '../files/preview'
import MermaidDiagram from './MermaidDiagram'
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
const ImageSourceContext = createContext<ImageSource | undefined>(undefined)
const ImageLinkContext = createContext(false)
// Keep renderer identities stable: replacing a renderer remounts its file links
// and discards any open preview, even when the report and source are unchanged.
const components: NonNullable<ComponentProps<typeof Markdown>['components']> = {
  a: function MarkdownLink({ href, children, node }) {
    const source = useContext(FileSourceContext)
    const content = <ImageLinkContext.Provider value={true}>{children}</ImageLinkContext.Provider>
    return typeof node?.properties.dataWakeitLocal === 'string' ? (
      <ResultFileLink href={node.properties.dataWakeitLocal} source={source}>
        {content}
      </ResultFileLink>
    ) : href ? (
      <a href={href} target="_blank" rel="noopener noreferrer">
        {content}
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
  img: function MarkdownImage({ src, alt, node }) {
    const source = useContext(ImageSourceContext)
    const linked = useContext(ImageLinkContext)
    return typeof node?.properties.dataWakeitLocal === 'string' ? (
      <ResultImage
        key={`${source?.runId}:${source?.nodeId}:${source?.document}:${source?.revision}:${node.properties.dataWakeitLocal}`}
        href={node.properties.dataWakeitLocal}
        alt={alt ?? ''}
        source={source}
        linked={linked}
      />
    ) : src ? (
      <img src={src} alt={alt ?? ''} loading="lazy" referrerPolicy="no-referrer" />
    ) : (
      <span>{alt}</span>
    )
  },
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
  imageSource = source,
}: {
  text: string
  source?: FileSource
  imageSource?: ImageSource
}) {
  return (
    <FileSourceContext.Provider value={source}>
      <ImageSourceContext.Provider value={imageSource}>
        <Markdown
          remarkPlugins={remarkPlugins}
          rehypePlugins={rehypePlugins}
          components={components}
        >
          {text.replace(/\x1b\[[0-9;]*m/g, '')}
        </Markdown>
      </ImageSourceContext.Provider>
    </FileSourceContext.Provider>
  )
})
