import { Modal, Steps, Progress, Typography } from 'antd';
import type { DeployProgress, DeployType } from '../types';
import { DEPLOY_STEPS, UPGRADE_STEPS, getDeployStepIndex } from '../constants';

const { Text } = Typography;

interface DeployProgressModalProps {
  open: boolean;
  instanceName: string;
  deployType: DeployType;
  progress: DeployProgress | null;
}

export function DeployProgressModal({
  open,
  instanceName,
  deployType,
  progress,
}: DeployProgressModalProps) {
  const isVersionChange = deployType === 'upgrade' || deployType === 'downgrade';
  const steps = isVersionChange ? UPGRADE_STEPS : DEPLOY_STEPS;
  const currentStep = progress ? getDeployStepIndex(progress.step, isVersionChange) : 0;

  const titleLabel =
    deployType === 'downgrade' ? '降级' : deployType === 'upgrade' ? '升级' : '部署';

  return (
    <Modal
      title={`${titleLabel}实例: ${instanceName}`}
      open={open}
      footer={null}
      closable={false}
      mask={{ closable: false }}
      width={500}
    >
      <Steps
        orientation="vertical"
        current={currentStep}
        items={steps.map((step) => ({ title: step.title }))}
        size="small"
        style={{ marginBottom: 24 }}
      />

      {progress && (
        <>
          <div style={{ overflow: 'hidden' }}>
            <Progress
              percent={progress.progress}
              status={
                progress.step === 'error'
                  ? 'exception'
                  : progress.step === 'done'
                    ? 'success'
                    : 'active'
              }
            />
          </div>
          <Text
            type={progress.step === 'error' ? 'danger' : 'secondary'}
            style={{ display: 'block', marginTop: 8, textAlign: 'center' }}
          >
            {progress.message}
          </Text>
        </>
      )}
    </Modal>
  );
}
