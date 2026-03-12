type ProgressStatus = 'normal' | 'active' | 'success' | 'exception'

interface CalcProgressParams {
  includeDdl: boolean
  includeData: boolean
  completedSteps: number
  hasError: boolean
}

interface CalcProgressResult {
  percent: number
  status: ProgressStatus
}

export function calcProgress({ includeDdl, includeData, completedSteps, hasError }: CalcProgressParams): CalcProgressResult {
  const totalSteps = (includeDdl ? 1 : 0) + (includeData ? 1 : 0)
  if (totalSteps === 0) {
    return { percent: 0, status: 'normal' }
  }

  const percent = Math.min(100, Math.round((completedSteps / totalSteps) * 100))

  if (hasError) {
    return { percent, status: 'exception' }
  }

  const status: ProgressStatus = percent >= 100 ? 'success' : 'active'

  return { percent, status }
}
