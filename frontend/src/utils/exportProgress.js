export function calcProgress({ includeDdl, includeData, completedSteps, hasError }) {
  const totalSteps = (includeDdl ? 1 : 0) + (includeData ? 1 : 0)
  if (totalSteps === 0) {
    return { percent: 0, status: 'normal' }
  }

  if (hasError) {
    return { percent: 100, status: 'exception' }
  }

  const percent = Math.min(100, Math.round((completedSteps / totalSteps) * 100))
  let status = 'active'
  if (percent >= 100) {
    status = 'success'
  }

  return { percent, status }
}
