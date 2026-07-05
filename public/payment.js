// Opens Razorpay Checkout with the order data embedded on the form,
// then submits the signed result back to /checkout/verify.
(function () {
  var form = document.getElementById('pay-data');
  var btn = document.getElementById('pay-btn');
  if (!form || !btn || typeof Razorpay === 'undefined') return;

  function openCheckout() {
    var rzp = new Razorpay({
      key: form.dataset.key,
      order_id: form.dataset.rzpOrder,
      amount: form.dataset.amount,
      currency: 'INR',
      name: "Sonna's Patisserie",
      description: 'Order ' + form.dataset.orderNumber,
      prefill: { name: form.dataset.name, contact: form.dataset.phone },
      theme: { color: '#DFA32B' },
      handler: function (response) {
        form.razorpay_payment_id.value = response.razorpay_payment_id;
        form.razorpay_signature.value = response.razorpay_signature;
        form.submit();
      }
    });
    rzp.open();
  }

  btn.addEventListener('click', openCheckout);
  openCheckout();
})();
