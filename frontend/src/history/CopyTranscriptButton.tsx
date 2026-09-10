import { Check, Copy } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { messageFrom } from '../app/formatting'
import { copyText } from '../tauri'

export function CopyTranscriptButton({ text, onError, iconOnly = false }: {
  text: string
  onError?: (message: string) => void
  iconOnly?: boolean
}) {
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const mountedRef = useRef(true)
  const copyVersionRef = useRef(0)
  const feedbackTimeoutRef = useRef<number | null>(null)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      copyVersionRef.current += 1
      if (feedbackTimeoutRef.current !== null) {
        window.clearTimeout(feedbackTimeoutRef.current)
        feedbackTimeoutRef.current = null
      }
    }
  }, [])

  const copy = async () => {
    if (!mountedRef.current) return
    const version = ++copyVersionRef.current
    if (feedbackTimeoutRef.current !== null) {
      window.clearTimeout(feedbackTimeoutRef.current)
      feedbackTimeoutRef.current = null
    }
    setCopied(false)
    setError(null)
    try {
      await copyText(text)
      if (!mountedRef.current || copyVersionRef.current !== version) return
      setCopied(true)
      feedbackTimeoutRef.current = window.setTimeout(() => {
        feedbackTimeoutRef.current = null
        if (mountedRef.current && copyVersionRef.current === version) setCopied(false)
      }, 1200)
    } catch (reason) {
      if (mountedRef.current && copyVersionRef.current === version) {
        const message = messageFrom(reason)
        if (onError) onError(message)
        else setError(message)
      }
    }
  }
  return (
    <>
      <button className={iconOnly ? 'icon-button' : 'secondary-button compact-button'} type="button" onClick={() => void copy()} aria-label={copied ? 'Copied transcript' : 'Copy transcript'}>
        {copied ? <Check size={17} aria-hidden="true" /> : <Copy size={17} aria-hidden="true" />}
        {!iconOnly ? <span aria-live="polite">{copied ? 'Copied transcript' : 'Copy transcript'}</span> : null}
      </button>
      {error ? <p role="alert">{error}</p> : null}
    </>
  )
}
