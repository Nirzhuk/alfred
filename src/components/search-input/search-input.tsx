import {
  forwardRef,
  useImperativeHandle,
  useRef,
  type InputHTMLAttributes,
} from "react";
import { Icon } from "../icon";
import "./search-input.css";

export type SearchInputProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "value" | "children"
> & {
  "aria-label": string;
  value: string;
  onClear?: () => void;
  clearLabel?: string;
  shortcut?: string;
  containerClassName?: string;
};

export const SearchInput = forwardRef<HTMLInputElement, SearchInputProps>(
  function SearchInput(
    {
      containerClassName,
      className,
      value,
      onClear,
      clearLabel = "Clear search",
      shortcut,
      disabled,
      readOnly,
      type = "search",
      ...props
    },
    ref,
  ) {
    const inputRef = useRef<HTMLInputElement>(null);
    useImperativeHandle(ref, () => inputRef.current!, []);
    return (
      <div
        className={["ui-search", containerClassName].filter(Boolean).join(" ")}
        data-disabled={disabled || undefined}
      >
        <Icon name="magnifying-glass" size={18} />
        <input
          {...props}
          ref={inputRef}
          type={type}
          className={["ui-search-input", className].filter(Boolean).join(" ")}
          value={value}
          disabled={disabled}
          readOnly={readOnly}
        />
        {value && onClear && !readOnly ? (
          <button
            type="button"
            className="ui-search-clear"
            aria-label={clearLabel}
            disabled={disabled}
            onClick={() => {
              onClear();
              inputRef.current?.focus();
            }}
          >
            <Icon name="x" size={14} />
          </button>
        ) : !value && shortcut ? (
          <kbd aria-hidden="true">{shortcut}</kbd>
        ) : null}
      </div>
    );
  },
);
