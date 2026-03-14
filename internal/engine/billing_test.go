package engine

import (
	"testing"
	"time"
)

func TestBillingCurrentMonth(t *testing.T) {
	bc := BillingCycle{ResetDay: 15}

	tests := []struct {
		name string
		date time.Time
		want string
	}{
		{"after reset day", time.Date(2026, 3, 20, 0, 0, 0, 0, time.UTC), "2026-03"},
		{"on reset day", time.Date(2026, 3, 15, 0, 0, 0, 0, time.UTC), "2026-03"},
		{"before reset day", time.Date(2026, 3, 10, 0, 0, 0, 0, time.UTC), "2026-02"},
		{"jan before reset", time.Date(2026, 1, 5, 0, 0, 0, 0, time.UTC), "2025-12"},
		{"dec after reset", time.Date(2025, 12, 20, 0, 0, 0, 0, time.UTC), "2025-12"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := bc.CurrentMonth(tt.date)
			if got != tt.want {
				t.Errorf("CurrentMonth(%v) = %s, want %s", tt.date, got, tt.want)
			}
		})
	}
}

func TestBillingResetDay1(t *testing.T) {
	bc := BillingCycle{ResetDay: 1}

	// On the 1st, should be current month
	got := bc.CurrentMonth(time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC))
	if got != "2026-03" {
		t.Errorf("reset day 1, march 1 = %s, want 2026-03", got)
	}

	// On the 28th, should still be current month
	got = bc.CurrentMonth(time.Date(2026, 3, 28, 0, 0, 0, 0, time.UTC))
	if got != "2026-03" {
		t.Errorf("reset day 1, march 28 = %s, want 2026-03", got)
	}
}

func TestBillingShouldReset(t *testing.T) {
	bc := BillingCycle{ResetDay: 1}

	now := time.Date(2026, 4, 5, 0, 0, 0, 0, time.UTC)
	if !bc.ShouldReset("2026-03", now) {
		t.Error("should reset when month changed")
	}
	if bc.ShouldReset("2026-04", now) {
		t.Error("should not reset when month is current")
	}
}

func TestBillingDaysRemaining(t *testing.T) {
	bc := BillingCycle{ResetDay: 15}

	// 10 days before reset
	days := bc.DaysRemaining(time.Date(2026, 3, 5, 0, 0, 0, 0, time.UTC))
	if days < 10 || days > 11 {
		t.Errorf("days remaining = %d, want ~10", days)
	}

	// After reset day, next month
	days = bc.DaysRemaining(time.Date(2026, 3, 20, 0, 0, 0, 0, time.UTC))
	if days < 25 || days > 27 {
		t.Errorf("days remaining after reset = %d, want ~26", days)
	}
}
