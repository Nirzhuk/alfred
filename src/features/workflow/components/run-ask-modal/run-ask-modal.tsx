import { useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import type { AskField } from "../../ask-fields";

type Props = {
  fields: AskField[];
  onRun: (answers: Record<string, string>) => void;
  onClose: () => void;
};

/** Collects `{{ask:…}}` answers before a manual run. */
export function RunAskModal({ fields, onRun, onClose }: Props) {
  const [answers, setAnswers] = useState<Record<string, string>>(() =>
    Object.fromEntries(fields.map((f) => [f.name, f.defaultValue])),
  );

  return (
    <Modal
      size="md"
      className="compact-form-modal"
      onClose={onClose}
      labelledBy="run-ask-modal-title"
      describedBy="run-ask-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="play" size={20} />
          </span>
        }
        title="Run workflow"
        titleId="run-ask-modal-title"
        description="Fill in this run's inputs."
        descriptionId="run-ask-modal-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />

      <form
        className="compact-form-modal-form"
        onSubmit={(e) => {
          e.preventDefault();
          onRun(answers);
        }}
      >
        <div className="compact-form-modal-body run-ask-fields">
          {fields.map((field, index) => (
            <label key={field.name} className="field">
              <span>{field.name}</span>
              <textarea
                autoFocus={index === 0}
                rows={2}
                value={answers[field.name] ?? ""}
                onChange={(e) =>
                  setAnswers((prev) => ({ ...prev, [field.name]: e.target.value }))
                }
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    e.currentTarget.form?.requestSubmit();
                  }
                }}
              />
            </label>
          ))}
        </div>

        <footer className="compact-form-modal-footer">
          <button type="submit" className="primary">
            Run
          </button>
        </footer>
      </form>
    </Modal>
  );
}
