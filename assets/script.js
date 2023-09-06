document.addEventListener("DOMContentLoaded", function () {
    const textInput = document.getElementById("textInput");
    const textArea = document.getElementById("textArea");
    const submitButton = document.getElementById("submitButton");
    const responseTable = document.getElementById("responseTable").getElementsByTagName('tbody')[0];

    submitButton.addEventListener("click", function () {
        // Get the text input value and text from the textarea
        const queryText = textInput.value;
        const requestBody = textArea.value;

        // Replace 'your-api-url' with the actual API endpoint
        const apiUrl = "/api";

        // Create the request body as a JSON object
        const bodyData = {
            query: queryText,
            content: requestBody,
        };

        // Perform the API request with a POST request
        fetch(apiUrl, {
            method: "POST",
            headers: {
                "Content-Type": "application/json",
            },
            body: JSON.stringify(bodyData),
        })
        .then(response => response.json()) // Assuming the API returns JSON
        .then(data => {
            // Clear previous results
            responseTable.innerHTML = "";

            // Iterate through the API response data and populate the table
            data.forEach(item => {
                const row = responseTable.insertRow();
                const idCell = row.insertCell(0);
                const nameCell = row.insertCell(1);

                idCell.textContent = item.id;
                nameCell.textContent = item.name;
                // Add more cells and data fields as needed
            });
        })
        .catch(error => {
            console.error("API request error:", error);
            // Handle errors here
        });
    });
});

