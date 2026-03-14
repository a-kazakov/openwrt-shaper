package engine

import (
	"fmt"
	"time"
)

// BillingCycle tracks the current billing period.
type BillingCycle struct {
	ResetDay int
}

// CurrentMonth returns the billing month string (e.g., "2026-03") for the given time.
func (bc *BillingCycle) CurrentMonth(now time.Time) string {
	year, month, day := now.Date()
	if day < bc.ResetDay {
		// Before reset day: we're still in the previous billing month
		month--
		if month < 1 {
			month = 12
			year--
		}
	}
	return fmt.Sprintf("%d-%02d", year, int(month))
}

// ShouldReset returns true if the stored billing month differs from the current one.
func (bc *BillingCycle) ShouldReset(storedMonth string, now time.Time) bool {
	return storedMonth != bc.CurrentMonth(now)
}

// DaysRemaining returns the number of days until the next billing reset.
func (bc *BillingCycle) DaysRemaining(now time.Time) int {
	year, month, day := now.Date()
	var nextReset time.Time
	if day < bc.ResetDay {
		nextReset = time.Date(year, month, bc.ResetDay, 0, 0, 0, 0, now.Location())
	} else {
		nextMonth := month + 1
		nextYear := year
		if nextMonth > 12 {
			nextMonth = 1
			nextYear++
		}
		nextReset = time.Date(nextYear, nextMonth, bc.ResetDay, 0, 0, 0, 0, now.Location())
	}
	return int(nextReset.Sub(now).Hours()/24) + 1
}
