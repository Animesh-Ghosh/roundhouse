# frozen_string_literal: true

# Markdown for the docs pages: CommonMark with GitHub's extensions (commonmarker,
# i.e. comrak), the dialect GitHub and src/guide.rs (pulldown-cmark) render these
# files in. Selected by `markdown: RoundhouseCommonmark` in _config.yml.
#
# Code blocks are highlighted with Rouge, in the markup Jekyll's own highlighter
# produces, so docs/assets/rouge.css applies unchanged.

require "cgi"
require "set"
require "commonmarker"
require "rouge"

module Jekyll
  module Converters
    class Markdown
      class RoundhouseCommonmark
        OPTIONS = {
          render: { unsafe: true, github_pre_lang: true, hardbreaks: false },
          extension: { table: true, strikethrough: true, autolink: true, tasklist: true, header_ids: "" }
        }.freeze

        def initialize(config)
          @config = config
        end

        # Raw HTML a doc may use on purpose. Any other tag in prose is shown as
        # text, like GitHub's tag filter but stricter: an unclosed <textarea> or
        # <script> from a misparsed code span would otherwise swallow the page.
        PROSE_TAGS = %w[
          a abbr b blockquote br code dd del details div dl dt em h1 h2 h3 h4 h5 h6 hr i img input
          kbd li mark ol p pre q s samp section small span strong sub summary sup table tbody td
          tfoot th thead tr u ul var
        ].to_set.freeze

        def convert(content)
          html = Commonmarker.to_html(content, options: OPTIONS, plugins: { syntax_highlighter: nil })
          html = html.gsub(%r{<a href="#[^"]*"[^>]*class="anchor"[^>]*></a>}, "")
          highlight(escape_unknown_tags(html))
        end

        private

        # Leaves <pre> and <code> contents alone (already escaped text). Allowed tags
        # lose event-handler attributes and javascript: URLs.
        def escape_unknown_tags(html)
          html.split(%r{(<pre\b.*?</pre>|<code\b.*?</code>)}m).map.with_index do |part, i|
            next part if i.odd?
            part.gsub(%r{</?([a-zA-Z][\w-]*)\b[^>]*>}) do |tag|
              PROSE_TAGS.include?(Regexp.last_match(1).downcase) ? clean_attributes(tag) : CGI.escapeHTML(tag)
            end
          end.join
        end
        
        UNSAFE_ATTRIBUTE = /\s(?:on[a-z]+|style|srcdoc|formaction)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/i
        UNSAFE_URL = /\s(href|src)\s*=\s*(["']?)\s*(?:javascript|vbscript|data):[^"'\s>]*\2/i
        
        def clean_attributes(tag)
          tag.gsub(UNSAFE_ATTRIBUTE, "").gsub(UNSAFE_URL) { %( #{Regexp.last_match(1)}="#") }
        end

        def highlight(html)
          html = html.gsub(%r{<pre lang="([^"]+)"><code>(.*?)</code></pre>}m) do
            lang = Regexp.last_match(1)
            code = CGI.unescapeHTML(Regexp.last_match(2))
            lexer = Rouge::Lexer.find_fancy(lang, code) || Rouge::Lexers::PlainText.new
            block(Rouge::Formatters::HTML.new.format(lexer.lex(code)), lang)
          end
          html.gsub(%r{<pre><code>(.*?)</code></pre>}m) { block(Regexp.last_match(1), nil) }
        end

        def block(body, lang)
          klass = lang ? "language-#{CGI.escapeHTML(lang)} highlighter-rouge" : "highlighter-rouge"
          %(<div class="#{klass}"><div class="highlight"><pre class="highlight"><code>#{body}</code></pre></div></div>)
        end
      end
    end
  end
end
